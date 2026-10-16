// TAKE IT DOWN Act requests (src/takedown.mjs). A valid request removes what it names at once, through the legal lock every
// serving route reads; the files' hashes go on a blocklist that every upload path checks; identical copies are found and
// locked; the author is told without the requester being named; the requester gets a receipt with the case reference; a
// person upholds or restores; a step that fails is retried by the hourly sweep. Against the real Worker with fake storage.
//
// Source note: plain ASCII only. A character a check needs from outside ASCII is built with String.fromCharCode.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx, testPng } from './helpers/load_worker.mjs';
import { snapshotReviewed, captureR2Key } from '../src/moderation.mjs';
import { manifestKey, chunkKey } from '../src/world.mjs';
import { prepareUserImage } from '../src/user_images.mjs';
import { GuestAllowance } from '../src/guest.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';
import {
  parseLocation, checkTakedownRequest, renderMail, COPIES_LOCKED_PER_HASH_MAX, REQUESTS_PER_EMAIL_PER_DAY, REQUESTS_PER_ADDRESS_PER_DAY,
} from '../src/takedown.mjs';
import { routePolicy, PUBLIC, SESSION, FRESH } from '../src/route_policy.mjs';
import { REFERENCE_PATTERN } from '../src/case_reference.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const VIEWER = '55555555-5555-4555-8555-555555555555';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const WORLD = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const COPY_IMAGE = 'cccccccc-3333-4333-8333-cccccccccccc';
const COPY_PACKAGE = 'cccccccc-3333-4333-8333-ccccccccccc2';
const UNRELATED = 'dddddddd-4444-4444-8444-dddddddddddd';
const NOWHERE = 'eeeeeeee-5555-4555-8555-eeeeeeeeeeee';

// What the requester told us. None of it may reach an author.
const REQUESTER = { name: 'Rowan Quillfeather', email: 'rowan.quill@example.org', depicted: 'Juniper Ashgrove', description: 'A private photo of me used as the thumbnail, shared without my consent.' };
const SECRETS = ['Rowan', 'Quillfeather', 'rowan.quill', 'Juniper', 'Ashgrove', 'private photo', '203.0.113'];

const EM_DASH = String.fromCharCode(0x2014);
const sha = (b) => crypto.createHash('sha256').update(typeof b === 'string' ? b : Buffer.from(b)).digest('hex');

const PAK = new Uint8Array([1, 2, 3]);
const THUMB_PNG = testPng(1280, 720, 11);
const COVER_PNG = testPng(640, 360, 12);
const CAPTURE_PNG = testPng(1280, 720, 13);
const stored = async (png) => { const p = await prepareUserImage(png); assert.ok(p.ok, p.error); return p.bytes; };
const THUMB = await stored(THUMB_PNG);
const COVER = await stored(COVER_PNG);
const CAPTURE = await stored(CAPTURE_PNG);

const CHUNK = new Uint8Array([0x45, 0x43, 0x48, 0x4b, 2, 0, 0, 0, 1, 2, 3, 4]);
const H1 = sha('chunk one');
const H2 = sha('chunk two');
const entry = (h) => ({ blake3: h, cx: 0, cz: 0, count: 1, file: `Main/${h.slice(0, 8)}.echk`, size: CHUNK.length });
const worldManifest = (universe, hashes) => JSON.stringify({ format: 'echk', encoder_version: 2, chunk_size: 64, universe, engine_version: '1.0', start_space: 'Main', spaces: [{ name: 'Main', chunks: hashes.map(entry) }], assets: [] });
const WORLD_MANIFEST = worldManifest('Harbor', [H1]);
const SOURCE_MANIFEST = worldManifest('Harbor', [H1, H2]);

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

/// The decoded plain-text and HTML parts and the subject of a message sendNotice built.
function readMail(m) {
  const part = (type) => {
    const at = m.raw.indexOf(`Content-Type: ${type}; charset=utf-8`);
    const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
    return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8');
  };
  const subjectLine = /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
  const encoded = /^=\?UTF-8\?B\?(.*)\?=$/.exec(subjectLine);
  return { to: m.to, subject: encoded ? Buffer.from(encoded[1], 'base64').toString('utf8') : subjectLine, text: part('text/plain'), html: part('text/html') };
}

async function putListing(env, sim) {
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify({
    genre: 'all', max_players: 10, is_public: true, play_count: 0, favorite_count: 0, version: 1,
    published_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z', moderation: { status: 'approved', rating: 'all_ages' }, ...sim,
  }));
  await env.SOCIAL.put(`sim-author:${sim.author_id}:${sim.id}`, sim.id);
  await env.SOCIAL.put(`modcase:${sim.id}`, JSON.stringify({ sim_id: sim.id, author_id: sim.author_id, status: sim.moderation?.status || 'approved', created_at: '2026-01-01T00:00:00.000Z', updated_at: '2026-01-01T00:00:00.000Z', decision: { rating: 'all_ages' }, legal_hold: null, history: [] }));
}

/// A Gallery with: SIM (a .pak listing with a thumbnail, a cover, a review capture shown as a preview, and a website manifest,
/// all bound to its approval), WORLD (an .echk listing with a shared source), two listings that hold one of SIM's files byte
/// for byte, and one that holds nothing of it.
async function seeded({ copies = true } = {}) {
  const env = { ...fakeEnv(), __EmailMessage: StubMessage, SECURITY_MAILBOX: 'security@example.com' };
  env.mail = [];
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  for (const [id, username, email] of [[AUTHOR, 'maker-one', 'maker@example.com'], [OTHER, 'maker-two', 'other@example.com'], [ADMIN, 'staff', 'staff@example.com'], [VIEWER, 'viewer', 'viewer@example.com']]) {
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username, email }));
  }
  await env.USERS.put(`admin:${ADMIN}`, '1');

  await putListing(env, {
    id: SIM, name: 'Harbor Survey', description: 'A survey of the harbor.', tags: ['ocean'], author_id: AUTHOR, author_name: 'maker-one',
    moderation: { status: 'approved', rating: 'all_ages', featured: true }, r2_key: `universes/${SIM}/universe.pak`, format: 'pak', pak_sha256: sha(PAK), scene_size_bytes: PAK.length,
    thumbnail_url: `https://api.eustress.dev/api/simulations/${SIM}/thumbnail`, cover: { ext: 'png', width: 640, height: 360, bytes: COVER.length, updated_at: '2026-01-01T00:00:00.000Z' },
  });
  await env.SCENES.put(`universes/${SIM}/universe.pak`, PAK, { sha256: sha(PAK) });
  await env.SCENES.put(`thumbnails/${SIM}/thumb.png`, THUMB, { sha256: sha(THUMB) });
  await env.SCENES.put(`covers/${SIM}/cover.png`, COVER, { sha256: sha(COVER) });
  await env.SCENES.put(captureR2Key(SIM, 0, 'png', 0), CAPTURE, { sha256: sha(CAPTURE) });
  const snap = await snapshotReviewed(env, JSON.parse(await env.SOCIAL.get(`sim:${SIM}`)), 0);
  await env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ v: 1, ...snap, evidence_epoch: 0, by: 'test', at: '2026-01-01T00:00:00.000Z' }));
  await env.SCENES.put(`universes/${SIM}/website-manifest.json`, JSON.stringify({ namespace: 'harbor', values: { depth: 12 } }), { customMetadata: { publishHash: 'h1', namespace: 'harbor' } });
  await env.SOCIAL.put('website-ns:harbor', JSON.stringify({ sim_id: SIM, owner_id: AUTHOR }));

  const worldKey = manifestKey(WORLD, sha(WORLD_MANIFEST));
  const sourceKey = manifestKey(WORLD, sha(SOURCE_MANIFEST));
  await env.SCENES.put(worldKey, WORLD_MANIFEST, { sha256: sha(WORLD_MANIFEST) });
  await env.SCENES.put(sourceKey, SOURCE_MANIFEST, { sha256: sha(SOURCE_MANIFEST) });
  await env.SCENES.put(chunkKey(WORLD, H1), CHUNK);
  await env.SCENES.put(chunkKey(WORLD, H2), CHUNK);
  await putListing(env, {
    id: WORLD, name: 'Tide Model', author_id: OTHER, author_name: 'maker-two', format: 'echk', r2_key: worldKey, open_source: true,
    world: { manifest_key: worldKey, manifest_sha256: sha(WORLD_MANIFEST), source_manifest_key: sourceKey, source_manifest_sha256: sha(SOURCE_MANIFEST), start_space: 'Main', spaces: 1, chunks: 1, bytes: CHUNK.length },
  });

  if (copies) {
    await putListing(env, { id: COPY_IMAGE, name: 'Copy With The Same Picture', author_id: OTHER, author_name: 'maker-two', r2_key: `universes/${COPY_IMAGE}/universe.pak`, format: 'pak', thumbnail_url: `https://api.eustress.dev/api/simulations/${COPY_IMAGE}/thumbnail` });
    await env.SCENES.put(`universes/${COPY_IMAGE}/universe.pak`, new Uint8Array([7, 7, 7]), { sha256: sha(new Uint8Array([7, 7, 7])) });
    await env.SCENES.put(`thumbnails/${COPY_IMAGE}/thumb.png`, THUMB, { sha256: sha(THUMB) });
    await putListing(env, { id: COPY_PACKAGE, name: 'Copy With The Same Package', author_id: OTHER, author_name: 'maker-two', r2_key: `universes/${COPY_PACKAGE}/universe.pak`, format: 'pak', pak_sha256: sha(PAK) });
    await env.SCENES.put(`universes/${COPY_PACKAGE}/universe.pak`, PAK, { sha256: sha(PAK) });
  }
  await putListing(env, { id: UNRELATED, name: 'Bridge Loads', author_id: OTHER, author_name: 'maker-two', r2_key: `universes/${UNRELATED}/universe.pak`, format: 'pak' });
  await env.SCENES.put(`universes/${UNRELATED}/universe.pak`, new Uint8Array([9, 9]), { sha256: sha(new Uint8Array([9, 9])) });
  return env;
}

async function call(env, method, path, { who, body, raw, headers = {} } = {}) {
  const h = { ...headers };
  if (who) h.Authorization = `Bearer ${mintToken(who)}`;
  let payload;
  if (raw !== undefined) payload = raw;
  else if (body !== undefined) { payload = JSON.stringify(body); h['Content-Type'] = 'application/json'; }
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: payload }), env, ctx);
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { json = null; }
  return { status: res.status, json, text, headers: res.headers };
}

const takedownBody = (over = {}) => ({
  requester_name: REQUESTER.name, signature: REQUESTER.name, relationship: 'self', contact_email: REQUESTER.email,
  locations: [`https://eustress.dev/simulation/${SIM}`], description: REQUESTER.description, good_faith: true, ...over,
});
const file = (env, over = {}, { ip = '203.0.113.7', who } = {}) => call(env, 'POST', '/api/takedown/requests', { body: takedownBody(over), headers: { 'CF-Connecting-IP': ip }, who });

async function runCron(env) {
  const waits = [];
  await worker.scheduled({ scheduledTime: Date.parse('2026-10-08T05:00:00.000Z') }, env, { waitUntil: (p) => waits.push(p), passThroughOnException() {} });
  await Promise.all(waits);
}

const tdCases = (env) => [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('tdcase:'));
const caseOf = async (env, reference) => JSON.parse(await env.SOCIAL.get(`tdcase:${JSON.parse(await env.SOCIAL.get(`caseref:${reference}`)).id}`));

/// What each serving route does for SIM (and WORLD), as someone who is not its author.
async function servingOf(env) {
  const listed = (r, field) => (r.json?.[field] || []).some((s) => s.id === SIM);
  return {
    list: listed(await call(env, 'GET', '/api/simulations'), 'simulations'),
    gallery: listed(await call(env, 'GET', '/api/gallery'), 'simulations'),
    featured: listed(await call(env, 'GET', '/api/gallery/featured'), 'featured'),
    tags: ((await call(env, 'GET', '/api/simulations/tags')).json?.tags || []).some((t) => t.tag === 'ocean'),
    makers: ((await call(env, 'GET', '/api/makers')).json?.makers || []).some((m) => m.username === 'maker-one'),
    page: (await call(env, 'GET', `/api/simulations/${SIM}`)).status,
    page_signed_in: (await call(env, 'GET', `/api/simulations/${SIM}`, { who: VIEWER })).status,
    download: (await call(env, 'GET', `/api/simulations/${SIM}/download`)).status,
    play: (await call(env, 'POST', `/api/simulations/${SIM}/play`, { who: VIEWER })).status,
    social_play: (await call(env, 'POST', '/api/social/play', { who: VIEWER, body: { simulation_id: SIM } })).status,
    guest_play: (await call(env, 'POST', '/api/guest/play', { body: { sim_id: SIM, character: 'M' }, headers: { 'CF-Connecting-IP': '198.51.100.20' } })).status,
    catalog: (await call(env, 'GET', `/api/commerce/catalog/${SIM}`)).status,
    thumbnail: (await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`)).status,
    cover: (await call(env, 'GET', `/api/simulations/${SIM}/cover`)).status,
    previews: ((await call(env, 'GET', `/api/simulations/${SIM}/previews`)).json?.previews || []).length,
    preview: (await call(env, 'GET', `/api/simulations/${SIM}/preview/0`)).status,
    website_manifest: (await call(env, 'GET', `/api/simulation/${SIM}/manifest`)).status,
    website_manifest_by_name: (await call(env, 'GET', '/api/simulation/harbor/latest/manifest')).status,
    world_manifest: (await call(env, 'GET', `/api/simulations/${WORLD}/world/manifest`, { who: VIEWER })).status,
    world_source: (await call(env, 'GET', `/api/simulations/${WORLD}/world/source`, { who: VIEWER })).status,
    world_chunk: (await call(env, 'GET', `/api/simulations/${WORLD}/world/chunks/${H1}`, { who: VIEWER })).status,
  };
}

const SERVED = {
  list: true, gallery: true, featured: true, tags: true, makers: true, page: 200, page_signed_in: 200, download: 200, play: 200, social_play: 200, guest_play: 200,
  catalog: 200, thumbnail: 200, cover: 200, previews: 1, preview: 200, website_manifest: 200, website_manifest_by_name: 200, world_manifest: 200, world_source: 200, world_chunk: 200,
};
const REMOVED = {
  list: false, gallery: false, featured: false, tags: false, makers: false, page: 404, page_signed_in: 404, download: 403, play: 403, social_play: 404, guest_play: 404,
  catalog: 404, thumbnail: 404, cover: 404, previews: 0, preview: 404, website_manifest: 404, website_manifest_by_name: 404, world_manifest: 403, world_source: 403, world_chunk: 403,
};

const BOTH = [`https://eustress.dev/simulation/${SIM}`, `https://api.eustress.dev/api/simulations/${WORLD}/world/manifest`];

// -- removal ------------------------------------------------------------------------------------------------------------------

test('a valid request removes what it names at once, from every route that serves it, and keeps the files', async () => {
  const env = await seeded();
  assert.deepEqual(await servingOf(env), SERVED, 'control: before the request everything serves (and the Gallery has listed it, so its lock list is warm)');

  const before = Date.now();
  const filed = await file(env, { locations: BOTH });
  assert.equal(filed.status, 201, filed.text);
  assert.match(filed.json.reference, REFERENCE_PATTERN);
  assert.equal(filed.json.status, 'removed');
  assert.deepEqual(filed.json.locations.map((l) => l.result), ['removed', 'removed']);
  assert.equal(filed.headers.get('Cache-Control'), 'no-store');

  assert.deepEqual(await servingOf(env), REMOVED, 'after the request nothing of it serves');
  // Not even to its author, and still to an administrator: the files are evidence, kept as they were.
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: AUTHOR })).status, 403);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, { who: AUTHOR })).status, 404);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: ADMIN })).status, 200);
  for (const key of [`universes/${SIM}/universe.pak`, `thumbnails/${SIM}/thumb.png`, `covers/${SIM}/cover.png`, captureR2Key(SIM, 0, 'png', 0), manifestKey(WORLD, sha(WORLD_MANIFEST))]) {
    assert.ok(env.SCENES.objects.has(key), `${key} is kept`);
  }
  // The author cannot change the content under the lock.
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/space`, { who: AUTHOR, raw: new Uint8Array([4, 4]) })).status, 409);

  // Removal is recorded with its times: seconds after the request, far inside 48 hours.
  const rec = await caseOf(env, filed.json.reference);
  const gap = Date.parse(rec.removed_at) - Date.parse(rec.received_at);
  assert.ok(gap >= 0 && gap < 5000, `removed ${gap} ms after the request`);
  assert.ok(Date.parse(rec.received_at) >= before - 1000);
  assert.deepEqual(rec.pending, [], 'nothing is owed');
  // The moderation case agrees with the lock.
  const mod = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  assert.equal(mod.legal_hold.source, 'takedown');
  assert.equal(mod.legal_hold.category, 'ncii_or_real_person_sexual');
  assert.deepEqual(mod.legal_hold.takedowns, [rec.id]);
  assert.equal(mod.status, 'approved', 'its review status is left as it was');

  // A listing the request did not name serves as before.
  assert.equal((await call(env, 'GET', `/api/simulations/${UNRELATED}/download`)).status, 200);
});

test('a link to one object of a listing (its thumbnail, a preview, a chunk, a website manifest by name) removes that listing', () => {
  const hosts = new Set(['api.eustress.dev']);
  const cases = [
    [SIM, { listing_id: SIM, object: 'listing' }],
    [SIM.toUpperCase(), { listing_id: SIM, object: 'listing' }],
    [`https://eustress.dev/simulation/${SIM}`, { listing_id: SIM, object: 'listing' }],
    [`https://eustress.dev/play/${SIM}?x=1`, { listing_id: SIM, object: 'listing' }],
    [`https://api.eustress.dev/api/simulations/${SIM}/thumbnail`, { listing_id: SIM, object: 'thumbnail' }],
    [`https://api.eustress.dev/api/simulations/${SIM}/cover?v=17`, { listing_id: SIM, object: 'cover' }],
    [`https://api.eustress.dev/api/simulations/${SIM}/preview/3`, { listing_id: SIM, object: 'preview:3' }],
    [`https://api.eustress.dev/api/simulations/${SIM}/download`, { listing_id: SIM, object: 'package' }],
    [`https://api.eustress.dev/api/simulations/${WORLD}/world/chunks/${H1}`, { listing_id: WORLD, object: `chunk:${H1}` }],
    [`https://api.eustress.dev/api/simulations/${WORLD}/world/source`, { listing_id: WORLD, object: 'source_manifest' }],
    [`https://api.eustress.dev/api/simulation/harbor/latest/manifest`, { namespace: 'harbor', object: 'website_manifest' }],
    [`https://api.eustress.dev/api/simulation/${SIM}/manifest`, { listing_id: SIM, object: 'website_manifest' }],
    [`eustress-player://play/${SIM}`, { listing_id: SIM, object: 'listing' }],
    [`https://projects.eustress.dev/x?review=${SIM}`, { listing_id: SIM, object: 'listing' }],
  ];
  for (const [raw, want] of cases) assert.deepEqual(parseLocation(raw, hosts), want, raw);
  for (const raw of ['', 'not a link', 'https://example.com/simulation/' + SIM, 'ftp://eustress.dev/' + SIM, 'https://eustress.dev/gallery', 'https://evil-eustress.dev/simulation/' + SIM]) {
    assert.ok(parseLocation(raw, hosts).error, `${raw} is refused`);
  }
});

test('a website manifest named by its namespace resolves to its listing and is removed', async () => {
  const env = await seeded({ copies: false });
  const filed = await file(env, { locations: ['https://api.eustress.dev/api/simulation/harbor/latest/manifest'] });
  assert.equal(filed.status, 201, filed.text);
  assert.equal(filed.json.locations[0].result, 'removed');
  assert.equal((await call(env, 'GET', '/api/simulation/harbor/latest/manifest')).status, 404);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
});

// -- refusing a request -------------------------------------------------------------------------------------------------------

test('a request that fails validation is refused with every missing piece, and nothing is removed or filed', async () => {
  const env = await seeded({ copies: false });
  const empty = await call(env, 'POST', '/api/takedown/requests', { body: {}, headers: { 'CF-Connecting-IP': '203.0.113.7' } });
  assert.equal(empty.status, 400);
  assert.equal(empty.json.code, 'invalid_request');
  for (const field of ['requester_name', 'signature', 'relationship', 'contact_email', 'locations', 'description', 'good_faith']) {
    assert.ok(empty.json.missing.includes(field), `${field} is named`);
    assert.ok(empty.json.problems.some((p) => p.field === field && p.message.length > 10), `${field} says what is wrong`);
  }
  const partial = await file(env, { good_faith: 'yes', relationship: 'authorized_representative', contact_email: 'not-an-address', locations: ['https://example.com/x'] });
  assert.equal(partial.status, 400);
  assert.deepEqual(partial.json.missing.sort(), ['contact_email', 'depicted_name', 'good_faith', 'locations[0]'].sort());
  const notJson = await call(env, 'POST', '/api/takedown/requests', { raw: 'name=me', headers: { 'CF-Connecting-IP': '203.0.113.7' } });
  assert.equal(notJson.status, 400);

  assert.deepEqual(tdCases(env), [], 'no case was filed');
  assert.equal(env.SCENES.objects.has(`locks/legal/${SIM}`), false, 'no lock was written');
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200, 'the listing still serves');
  assert.equal(env.mail.length, 0, 'nobody was emailed');
});

test('the signature must be the requester\'s full name typed again', async () => {
  const env = await seeded({ copies: false });
  const wrong = await file(env, { signature: 'R. Quillfeather' });
  assert.equal(wrong.status, 400);
  assert.deepEqual(wrong.json.missing, ['signature']);
  const missing = await file(env, { signature: '  ' });
  assert.deepEqual(missing.json.missing, ['signature']);
  assert.deepEqual(tdCases(env), []);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200);
  // Case and spacing are not part of a name.
  const same = await file(env, { signature: '  rowan   QUILLFEATHER ' });
  assert.equal(same.status, 201, same.text);
});

test('the rate limit holds per contact email and per address, a refused request files nothing, and the answer says how to reach a person', async () => {
  const env = await seeded({ copies: false });
  for (let i = 0; i < REQUESTS_PER_EMAIL_PER_DAY; i++) assert.equal((await file(env, {}, { ip: `198.51.100.${i + 1}` })).status, 201, `request ${i + 1}`);
  const filed = tdCases(env).length;
  const sixth = await file(env, {}, { ip: '198.51.100.99' });
  assert.equal(sixth.status, 429);
  assert.equal(sixth.json.code, 'rate_limited');
  assert.match(sixth.json.error, /support@eustress\.dev/);
  assert.ok(Number(sixth.headers.get('Retry-After')) > 0);
  assert.equal(tdCases(env).length, filed, 'the refused request filed nothing');

  const env2 = await seeded({ copies: false });
  for (let i = 0; i < REQUESTS_PER_ADDRESS_PER_DAY; i++) assert.equal((await file(env2, { contact_email: `person${i}@example.org` }, { ip: '192.0.2.44' })).status, 201);
  assert.equal((await file(env2, { contact_email: 'another@example.org' }, { ip: '192.0.2.44' })).status, 429, 'one address, many emails');
  assert.equal((await file(env2, { contact_email: 'another@example.org' }, { ip: '192.0.2.45' })).status, 201, 'another address is not limited by it');

  // A burst is stopped by the platform limiter before the body is read, keyed apart from sign-in's.
  const env3 = await seeded({ copies: false });
  const keys = [];
  env3.AUTH_RATE_LIMITER = { limit: async ({ key }) => { keys.push(key); return { success: false }; } };
  const burst = await file(env3);
  assert.equal(burst.status, 429);
  assert.deepEqual(keys, ['takedown:203.0.113.7']);
  assert.deepEqual(tdCases(env3), []);
});

test('a location that names nothing on Eustress is filed for a person, and nothing is removed', async () => {
  const env = await seeded({ copies: false });
  const filed = await file(env, { locations: [`https://eustress.dev/simulation/${NOWHERE}`] });
  assert.equal(filed.status, 201);
  assert.equal(filed.json.status, 'under_review');
  assert.deepEqual(filed.json.locations, [{ location: `https://eustress.dev/simulation/${NOWHERE}`, result: 'not_found' }]);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200);
  const staff = env.mail.map(readMail).find((m) => m.to === 'security@example.com');
  assert.ok(staff, 'staff are told');
  const receipt = env.mail.map(readMail).find((m) => m.to === REQUESTER.email);
  assert.match(receipt.text, /could not find content at/);
});

// -- known identical copies -----------------------------------------------------------------------------------------------------

test('identical copies are found and locked: the same picture, and the same package, in other listings', async () => {
  const env = await seeded();
  assert.equal((await call(env, 'GET', `/api/simulations/${COPY_IMAGE}/download`)).status, 200, 'control');
  assert.equal((await call(env, 'GET', `/api/simulations/${COPY_PACKAGE}/download`)).status, 200, 'control');
  const filed = await file(env);
  assert.equal(filed.status, 201);
  for (const id of [COPY_IMAGE, COPY_PACKAGE]) {
    assert.equal((await call(env, 'GET', `/api/simulations/${id}/download`)).status, 403, `${id} is locked`);
    assert.equal((await call(env, 'GET', '/api/simulations')).json.simulations.some((s) => s.id === id), false, `${id} is not listed`);
  }
  assert.equal((await call(env, 'GET', `/api/simulations/${UNRELATED}/download`)).status, 200, 'a listing with none of the files is untouched');
  const rec = await caseOf(env, filed.json.reference);
  const copies = rec.targets.filter((t) => t.via === 'identical_copy');
  assert.deepEqual(copies.map((t) => [t.sim_id, t.matched_kind]).sort(), [[COPY_IMAGE, 'thumbnail'], [COPY_PACKAGE, 'package']].sort());
  // Their author is told, in the copy's own words.
  const told = env.mail.map(readMail).filter((m) => m.to === 'other@example.com');
  assert.equal(told.length, 2);
  for (const m of told) assert.match(m.text, /identical, byte for byte/);
});

test('past the cap, more copies of one file are recorded for a person and not locked', async () => {
  const env = await seeded({ copies: false });
  const ids = [];
  for (let i = 0; i < COPIES_LOCKED_PER_HASH_MAX + 1; i++) {
    const id = `f${String(i).padStart(7, '0')}-6666-4666-8666-ffffffffffff`;
    ids.push(id);
    await putListing(env, { id, name: `Stock ${i}`, author_id: OTHER, author_name: 'maker-two' });
    await env.SCENES.put(`thumbnails/${id}/thumb.png`, THUMB, { sha256: sha(THUMB) });
  }
  const filed = await file(env);
  const rec = await caseOf(env, filed.json.reference);
  assert.equal(rec.targets.filter((t) => t.via === 'identical_copy').length, COPIES_LOCKED_PER_HASH_MAX);
  assert.equal(rec.copy_candidates.length, 1);
  assert.equal(ids.filter((id) => env.SCENES.objects.has(`locks/legal/${id}`)).length, COPIES_LOCKED_PER_HASH_MAX);
});

// -- the blocklist on every upload path -----------------------------------------------------------------------------------------

test('a re-upload of a removed file is refused on every upload path, while other files go through', async () => {
  const env = await seeded({ copies: false });
  const filed = await file(env, { locations: BOTH });
  assert.equal(filed.status, 201);
  const up = (method, path, raw) => call(env, method, path, { who: OTHER, raw });
  const blocked = (r, what) => { assert.equal(r.status, 422, `${what}: ${r.text}`); assert.equal(r.json.code, 'content_blocked', what); };

  // The package (.pak). The control also sends the listing back to pending, where captures are taken.
  assert.equal((await up('PUT', `/api/simulations/${UNRELATED}/space`, new Uint8Array([5, 5, 5]))).status, 200, 'control: another package');
  blocked(await up('PUT', `/api/simulations/${UNRELATED}/space`, PAK), '.pak');
  // A Space package on its own.
  assert.equal((await up('PUT', `/api/simulations/${UNRELATED}/spaces/Main`, new Uint8Array([6, 6, 6]))).status, 200, 'control: another Space');
  blocked(await up('PUT', `/api/simulations/${UNRELATED}/spaces/Main`, PAK), 'Space package');
  // The thumbnail, as the original file (its metadata would be dropped, giving the stored bytes).
  assert.equal((await up('PUT', `/api/simulations/${UNRELATED}/thumbnail`, testPng(1280, 720, 99))).status, 200, 'control: another thumbnail');
  blocked(await up('PUT', `/api/simulations/${UNRELATED}/thumbnail`, THUMB_PNG), 'thumbnail');
  // The cover.
  assert.equal((await up('PUT', `/api/simulations/${UNRELATED}/cover`, testPng(640, 360, 98))).status, 200, 'control: another cover');
  blocked(await up('PUT', `/api/simulations/${UNRELATED}/cover`, COVER_PNG), 'cover');
  // A review capture.
  assert.equal((await up('PUT', `/api/simulations/${UNRELATED}/captures/1`, testPng(1280, 720, 97))).status, 200, 'control: another capture');
  blocked(await up('PUT', `/api/simulations/${UNRELATED}/captures/1`, CAPTURE_PNG), 'capture');
  // The world commit, with the chunks already stored for the listing.
  await env.SCENES.put(chunkKey(UNRELATED, H1), CHUNK);
  await env.SCENES.put(chunkKey(UNRELATED, H2), CHUNK);
  assert.equal((await call(env, 'POST', `/api/simulations/${UNRELATED}/world/commit`, { who: OTHER, body: { manifest_json: worldManifest('Other', [H1]), listing: { open_source: true } } })).status, 200, 'control: another world');
  blocked(await call(env, 'POST', `/api/simulations/${UNRELATED}/world/commit`, { who: OTHER, body: { manifest_json: WORLD_MANIFEST } }), 'world');
  blocked(await call(env, 'POST', `/api/simulations/${UNRELATED}/world/commit`, { who: OTHER, body: { manifest_json: SOURCE_MANIFEST, kind: 'source' } }), 'source world');

  // None of the refused bytes was stored, and each attempt is kept and reported.
  assert.equal(env.SCENES.objects.has(`thumbnails/${UNRELATED}/thumb.png`) && sha(env.SCENES.objects.get(`thumbnails/${UNRELATED}/thumb.png`).body) === sha(THUMB), false);
  assert.equal(sha(env.SCENES.objects.get(`universes/${UNRELATED}/universe.pak`).body), sha(new Uint8Array([5, 5, 5])));
  const attempts = [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('ncii-attempt:'));
  assert.equal(attempts.length, 7, 'one for each refusal above');
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'security@example.com' && /A blocked file was uploaded again/.test(m.subject)));
});

test('a multipart package that assembles to a removed file is refused and deleted, and the listing is left as it was', async () => {
  const control = await seeded({ copies: false });
  const created = await call(control, 'POST', `/api/simulations/${UNRELATED}/space/multipart/create`, { who: OTHER });
  assert.equal(created.status, 200);
  // The fake bucket assembles every multipart upload to the bytes 1, 2, 3: SIM's package.
  const ok = await call(control, 'POST', `/api/simulations/${UNRELATED}/space/multipart/complete`, { who: OTHER, body: { upload_id: created.json.upload_id, parts: [{ part_number: 1, etag: 'part-1' }] } });
  assert.equal(ok.status, 200, 'control: with no takedown the same upload goes through');

  const env = await seeded({ copies: false });
  assert.equal((await file(env)).status, 201);
  const before = await env.SOCIAL.get(`sim:${UNRELATED}`);
  const again = await call(env, 'POST', `/api/simulations/${UNRELATED}/space/multipart/create`, { who: OTHER });
  const refused = await call(env, 'POST', `/api/simulations/${UNRELATED}/space/multipart/complete`, { who: OTHER, body: { upload_id: again.json.upload_id, parts: [{ part_number: 1, etag: 'part-1' }] } });
  assert.equal(refused.status, 422, refused.text);
  assert.equal(refused.json.code, 'content_blocked');
  assert.equal(env.SCENES.objects.has(`universes/${UNRELATED}/universe.pak`), false, 'the assembled file is gone');
  assert.equal(await env.SOCIAL.get(`sim:${UNRELATED}`), before, 'the listing record is untouched');
});

// -- telling people -------------------------------------------------------------------------------------------------------------

test('the author is told with the reference and how to ask for a review, and nothing they see names the requester', async () => {
  const env = await seeded({ copies: false });
  const filed = await file(env, { relationship: 'authorized_representative', depicted_name: REQUESTER.depicted });
  assert.equal(filed.status, 201);
  const ref = filed.json.reference;

  const feed = await call(env, 'GET', '/api/notifications', { who: AUTHOR });
  const item = feed.json.items.find((i) => i.kind === 'takedown');
  assert.ok(item, 'a feed item');
  assert.equal(item.reference, ref);
  assert.equal(item.sim_id, SIM);
  assert.equal(item.event, 'removed');

  const mails = env.mail.map(readMail).filter((m) => m.to === 'maker@example.com');
  assert.equal(mails.length, 1);
  const [m] = mails;
  assert.match(m.subject, /Harbor Survey/);
  assert.ok(m.text.includes(ref), 'the reference');
  assert.match(m.text, /TAKE IT DOWN Act/);
  assert.match(m.text, /Ask for a review/);
  assert.match(m.text, /support@eustress\.dev/);
  assert.match(m.text, /Nothing is restored automatically; a person decides/);

  const cases = await call(env, 'GET', '/api/account/cases', { who: AUTHOR });
  const seen = JSON.stringify([feed.json, m.subject, m.text, m.html, cases.json]);
  for (const secret of SECRETS) assert.equal(seen.includes(secret), false, `the author never sees "${secret}"`);
  assert.equal(seen.includes(EM_DASH), false, 'no em dash in what the author reads');
});

test('the requester gets a receipt with the case reference, and can follow it by the reference', async () => {
  const env = await seeded({ copies: false });
  const filed = await file(env);
  const ref = filed.json.reference;
  assert.match(ref, REFERENCE_PATTERN);
  assert.equal(filed.json.status_url, `https://api.eustress.dev/api/cases/${ref}`);

  const receipts = env.mail.map(readMail).filter((m) => m.to === REQUESTER.email);
  assert.equal(receipts.length, 1);
  const [r] = receipts;
  assert.ok(r.subject.includes(ref), 'the reference is in the subject');
  assert.ok(r.text.includes(ref), 'and in the body');
  assert.ok(r.text.includes(`https://eustress.dev/cases/${ref}`), 'with the status page');
  assert.match(r.text, /We removed the content at 1 of the 1 location you gave/);
  assert.equal(r.text.includes(REQUESTER.description), false, 'the description is not echoed into email');
  assert.equal(r.text.includes(EM_DASH), false);

  const status = await call(env, 'GET', `/api/cases/${ref}`);
  assert.equal(status.status, 200);
  assert.equal(status.json.status, 'removed');
});

test('content the request says shows a minor is locked the same way, flagged for staff, preserved without end, and its author is not told', async () => {
  const env = await seeded({ copies: false });
  const filed = await file(env, { depicts_minor: true });
  assert.equal(filed.status, 201);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  const rec = await caseOf(env, filed.json.reference);
  assert.equal(rec.possible_csam, true);
  const mod = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  assert.equal(mod.legal_hold.category, 'csam_or_minor_sexualization');
  assert.equal(mod.legal_hold.possible_csam, true);
  assert.equal(mod.legal_hold.escalated, true);
  assert.equal(mod.legal_hold.never_delete, true);
  assert.equal(mod.legal_hold.preserve_until, null);
  assert.ok(JSON.parse(await env.SOCIAL.get(`ncii-hash:${sha(PAK)}`)).possible_csam);
  // Staff hear about it at once, by its name; the author hears nothing and their history does not list it.
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'security@example.com' && /Possible CSAM/.test(m.subject)));
  assert.equal(env.mail.map(readMail).filter((m) => m.to === 'maker@example.com').length, 0);
  assert.equal((await call(env, 'GET', '/api/notifications', { who: AUTHOR })).json.items.filter((i) => i.kind === 'takedown').length, 0);
  assert.equal((await call(env, 'GET', '/api/account/cases', { who: AUTHOR })).json.cases.filter((c) => c.kind.startsWith('takedown')).length, 0);
  // The files stay.
  assert.ok(env.SCENES.objects.has(`universes/${SIM}/universe.pak`));
});

test('every mail renders without an em dash and the author mails carry only what is passed to them', () => {
  const d = { name: 'Harbor Survey', at: '2026-10-08T17:00:00.000Z', reference: 'EC-0000-1111-2222-3333-4444', site: 'https://eustress.dev', received_at: '2026-10-08T17:00:00.000Z', removed_at: '2026-10-08T17:00:01.000Z', removed: 1, total: 1, not_found: [], pending: [], status_page: 'https://eustress.dev/cases/EC-0000-1111-2222-3333-4444', decided_at: '2026-10-09T17:00:00.000Z', outcome: 'confirmed', id: 'x', possible_csam: false, named: 1, copies: 0, admin_url: 'https://api.eustress.dev/api/admin/takedowns/x', errors: {}, deadline: '2026-10-10T17:00:00.000Z', reviews: ['author'], references: [], sha256: 'a'.repeat(64) };
  for (const kind of ['author_removed', 'author_removed_copy', 'author_restored', 'author_upheld', 'receipt', 'requester_decided', 'staff_new', 'staff_csam', 'staff_failure', 'staff_relocked', 'staff_review', 'staff_attempt', 'staff_candidates']) {
    const m = renderMail(kind, d);
    for (const part of [m.subject, m.text, m.html]) assert.equal(part.includes(EM_DASH), false, `${kind} has no em dash`);
    assert.ok(m.text.length > 40 && m.html.includes('<html'), kind);
  }
  assert.throws(() => renderMail('nope', d));
});

// -- a person's review --------------------------------------------------------------------------------------------------------

test('only an administrator reviews, with a note; confirming keeps the removal, and the sweep never restores it', async () => {
  const env = await seeded({ copies: false });
  const filed = await file(env);
  const rec = await caseOf(env, filed.json.reference);
  assert.equal((await call(env, 'GET', '/api/admin/takedowns', { who: VIEWER })).status, 403);
  assert.equal((await call(env, 'POST', `/api/admin/takedowns/${rec.id}/invalid`, { who: AUTHOR, body: { note: 'I am the author and I say it is fine to restore.' } })).status, 403);
  assert.equal((await call(env, 'POST', `/api/admin/takedowns/${rec.id}/confirm`, { who: ADMIN, body: { note: 'short' } })).status, 400);

  const queue = await call(env, 'GET', '/api/admin/takedowns', { who: ADMIN });
  assert.equal(queue.status, 200);
  assert.deepEqual(queue.json.items.map((i) => i.reference), [filed.json.reference]);
  const full = await call(env, 'GET', `/api/admin/takedowns/${rec.id}`, { who: ADMIN });
  assert.equal(full.json.case.request.requester_name, REQUESTER.name, 'staff see the request');
  assert.equal(full.headers.get('Cache-Control'), 'private, no-store');

  const confirmed = await call(env, 'POST', `/api/admin/takedowns/${rec.id}/confirm`, { who: ADMIN, body: { note: 'The requester is the person shown; consent was not given.' } });
  assert.equal(confirmed.status, 200, confirmed.text);
  assert.equal(confirmed.json.case.status, 'confirmed');
  await runCron(env);
  await runCron(env);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403, 'still removed');
  assert.ok(await env.SOCIAL.get(`ncii-hash:${sha(PAK)}`), 'still blocked');
  assert.equal((await call(env, 'GET', `/api/cases/${filed.json.reference}`)).json.status, 'decided');
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'maker@example.com' && /upheld/.test(m.subject)));
  assert.ok(env.mail.map(readMail).some((m) => m.to === REQUESTER.email && /reviewed/.test(m.subject)));
  const audit = [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v));
  assert.ok(audit.some((a) => a.action === 'TAKEDOWN_CONFIRMED' && a.admin_id === ADMIN));
});

test('a person marks a request invalid: the listing and its copies serve again, and their files can be published again', async () => {
  const env = await seeded();
  const filed = await file(env, { locations: BOTH });
  const rec = await caseOf(env, filed.json.reference);
  assert.deepEqual(await servingOf(env), REMOVED);
  const hashes = rec.hashes.map((h) => h.sha256);
  assert.ok(hashes.includes(sha(PAK)) && hashes.includes(sha(THUMB)) && hashes.includes(sha(COVER)) && hashes.includes(sha(CAPTURE)) && hashes.includes(sha(WORLD_MANIFEST)) && hashes.includes(sha(SOURCE_MANIFEST)), 'every file was blocked');

  const restored = await call(env, 'POST', `/api/admin/takedowns/${rec.id}/invalid`, { who: ADMIN, body: { note: 'The listing shows a landscape; nothing in it matches the request.' } });
  assert.equal(restored.status, 200, restored.text);
  assert.deepEqual(restored.json.incomplete, []);
  assert.deepEqual(await servingOf(env), SERVED, 'everything serves again');
  for (const id of [COPY_IMAGE, COPY_PACKAGE]) assert.equal((await call(env, 'GET', `/api/simulations/${id}/download`)).status, 200, `${id} is restored`);
  for (const h of hashes) assert.equal(await env.SOCIAL.get(`ncii-hash:${h}`), null, `${h} is off the blocklist`);
  assert.equal(JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`)).legal_hold, null);
  assert.equal((await call(env, 'PUT', `/api/simulations/${UNRELATED}/thumbnail`, { who: OTHER, raw: THUMB_PNG })).status, 200, 'the picture can be published again');
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'maker@example.com' && /available again/.test(m.subject)));
  // Decided, and staying that way: a second decision is refused.
  assert.equal((await call(env, 'POST', `/api/admin/takedowns/${rec.id}/confirm`, { who: ADMIN, body: { note: 'Changing my mind after all.' } })).status, 409);
});

test('a lock moved by anything but this review is put back while the takedown is in force', async () => {
  const env = await seeded({ copies: false });
  await file(env);
  // As a release of a moderation hold on the same listing would: the lock deleted, the case's hold cleared.
  env.SCENES.objects.delete(`locks/legal/${SIM}`);
  const mod = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ ...mod, legal_hold: null }));
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200, 'the hole the sweep closes');
  await runCron(env);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  assert.equal(JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`)).legal_hold.source, 'takedown');
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'security@example.com' && /put back/.test(m.subject)));
});

// -- no silent failure ----------------------------------------------------------------------------------------------------------

test('a removal step that fails is recorded, staff are told, and the hourly sweep retries it until it succeeds', async () => {
  const env = await seeded({ copies: false });
  const realPut = env.SCENES.put;
  env.SCENES.put = async (key, value, meta) => {
    if (key.startsWith('locks/legal/')) throw new Error('R2 is unavailable');
    return realPut(key, value, meta);
  };
  const filed = await file(env);
  assert.equal(filed.status, 201, filed.text);
  assert.equal(filed.json.status, 'received');
  assert.deepEqual(filed.json.locations.map((l) => l.result), ['pending']);
  const rec = await caseOf(env, filed.json.reference);
  assert.ok(rec.pending.includes(`lock:${SIM}`));
  assert.match(rec.step_errors[`lock:${SIM}`].error, /R2 is unavailable/);
  assert.ok(await env.SOCIAL.get(`tdpending:${rec.id}`));
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'security@example.com' && /step is failing/.test(m.subject)), 'staff are told at once');

  // Still failing at the next sweep: still owed, staff told again.
  const told = env.mail.length;
  await runCron(env);
  assert.ok((await caseOf(env, filed.json.reference)).pending.includes(`lock:${SIM}`));
  assert.ok(env.mail.length > told);

  env.SCENES.put = realPut;
  await runCron(env);
  const after = await caseOf(env, filed.json.reference);
  assert.deepEqual(after.pending, []);
  assert.equal(after.status, 'removed');
  assert.ok(after.removed_at);
  assert.equal(await env.SOCIAL.get(`tdpending:${rec.id}`), null);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 403);
  assert.equal((await call(env, 'GET', '/api/simulations')).json.simulations.some((s) => s.id === SIM), false);
  assert.equal((await call(env, 'GET', `/api/cases/${filed.json.reference}`)).json.status, 'removed');
});

// -- the table ------------------------------------------------------------------------------------------------------------------

test('the routes are classed: the request public, the history session, the review an administrator\'s', () => {
  const expect = [
    ['POST', '/api/takedown/requests', PUBLIC], ['GET', '/api/cases/EC-0000-1111-2222-3333-4444', PUBLIC],
    ['GET', '/api/account/cases', SESSION], ['POST', '/api/account/cases/EC-0000-1111-2222-3333-4444/review', SESSION],
    ['GET', '/api/admin/takedowns', FRESH], ['POST', `/api/admin/takedowns/${SIM}/confirm`, FRESH], ['POST', `/api/admin/takedowns/${SIM}/invalid`, FRESH], ['POST', `/api/admin/takedowns/${SIM}/close`, FRESH],
  ];
  for (const [method, path, klass] of expect) {
    const r = routePolicy(method, path);
    assert.equal(r.class, klass, `${method} ${path}`);
    assert.equal(r.explicit, true, `${path} is named`);
  }
});

test('the request check keeps what was given, and the signature and good faith are required as given', () => {
  const ok = checkTakedownRequest(takedownBody({ locations: `https://eustress.dev/simulation/${SIM}` }));
  assert.deepEqual(ok.problems, []);
  assert.equal(ok.value.locations.length, 1, 'one location may be sent as a string');
  assert.equal(ok.value.depicts_minor, false);
  assert.deepEqual(checkTakedownRequest(takedownBody({ good_faith: 'true' })).problems.map((p) => p.field), ['good_faith']);
  assert.deepEqual(checkTakedownRequest(takedownBody({ locations: Array(21).fill(SIM) })).problems.map((p) => p.field), ['locations']);
  assert.deepEqual(checkTakedownRequest(takedownBody({ depicts_minor: 'yes' })).problems.map((p) => p.field), ['depicts_minor']);
});
