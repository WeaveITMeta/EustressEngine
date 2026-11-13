// Security probes for Delete Published Space, from the independent review (docs/architecture/DELETE_PUBLISHED_SPACE_SECURITY_REVIEW.md),
// copied from the reviewer's tests/zz_security_probes.test.mjs. Each probe failed on the code as reviewed and passes with its fix;
// a line changed because a fix changed what is expected is marked ADJUSTED (fix). M5 is McKale's decision and stays a todo.
// Helpers copied from tests/space_delete.test.mjs (lines 1-177 and the HOLDS table).
// Deleting a published Space (src/space_delete.mjs, src/space_delete_ops.mjs, src/space_delete_mark.mjs;
// docs/architecture/DELETE_PUBLISHED_SPACE.md, with McKale's decisions of 2026-11-12): the typed name and a six-digit code
// emailed to the verified, aged address; fifteen minutes, five attempts, once; three requests per listing an hour and five
// per account a day; nothing while DELETE_CODE_EMAIL is not "on"; no deletion while money is open; every hold refuses the
// start and defers the removal; ninety days to restore; the hourly job removes the rest in resumable pages and never touches
// money, evidence or cases; the R2 mark survives any KV rewrite; the code is never logged.
// The real Worker and the real Durable Objects run against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { loadWorker, workerModule, mintToken, fakeEnv, fakeKv, fakeBucket, fakeDoNamespace, ctx, testPng } from './helpers/load_worker.mjs';

const worker = await loadWorker();
// The modules as the loaded Worker sees them, so a direct call shares their state with the Worker's own.
const SD = await workerModule('space_delete.mjs');
const { AccountAuth, accountCall, accountState } = await workerModule('account_auth.mjs');
const { CommerceHub, Wallet } = await workerModule('commerce.mjs');
const { MARK_STATES } = await workerModule('space_delete_mark.mjs');

const MIN = 60 * 1000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const ORIGIN = 'https://api.eustress.dev';
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const STRANGER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM2 = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const SIM3 = 'cccccccc-3333-4333-8333-cccccccccccc';
const NAME = 'The Super Station';
const REF = 'EC-AAAA-BBBB-CCCC-DDDD-EEEE';
const IN_REVIEW = { error: 'This listing is being reviewed and cannot be changed right now', code: 'in_review' };
const NEUTRAL = { error: "This Space can't be deleted right now. Contact support@eustress.dev.", code: 'cannot_delete_now' };
const tokens = { author: mintToken(AUTHOR), stranger: mintToken(STRANGER), admin: mintToken(ADMIN) };

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

/// The text and HTML of a mail the Worker built, decoded.
function bodyOf(message) {
  return [...message.raw.matchAll(/Content-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)]
    .map((m) => Buffer.from(m[1].replace(/\r\n/g, ''), 'base64').toString())
    .join('\n');
}
const subjectOf = (m) => /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
const codeIn = (m) => /\n(\d{6})\n/.exec(bodyOf(m))?.[1] || null;

/// An R2 bucket that pages like R2 (limit, cursor, truncated), deletes a list of keys in one call, counts its calls, and can
/// be told to fail or to run a hook before a delete call.
function pagedBucket() {
  const b = fakeBucket();
  const base = { put: b.put, get: b.get, head: b.head };
  b.calls = { list: 0, delete: 0, put: 0, get: 0, head: 0 };
  b.beforeDelete = null;
  b.failDeleteAfter = Infinity;
  b.put = async (...a) => { b.calls.put++; return base.put(...a); };
  b.get = async (k) => { b.calls.get++; return base.get(k); };
  b.head = async (k) => { b.calls.head++; return base.head(k); };
  b.calls.pageDeletes = 0;
  b.delete = async (keys) => {
    if (b.beforeDelete) await b.beforeDelete(keys);
    // A failure is made for the job's page deletes (a list of keys), not for a lease being given back.
    if (Array.isArray(keys) && b.calls.pageDeletes >= b.failDeleteAfter) throw new Error('R2 is down');
    b.calls.delete++;
    if (Array.isArray(keys)) b.calls.pageDeletes++;
    for (const k of [].concat(keys)) b.objects.delete(k);
  };
  b.list = async ({ prefix = '', limit = 1000, cursor, include = [] } = {}) => {
    b.calls.list++;
    // R2's cursor is a position in key order: the next page starts after the last key listed, whatever was deleted since.
    const all = [...b.objects.keys()].filter((k) => k.startsWith(prefix) && (!cursor || k > cursor)).sort();
    const page = all.slice(0, Math.min(limit, 1000));
    const more = all.length > page.length;
    return {
      objects: page.map((key) => ({ key, size: b.objects.get(key).body.byteLength, ...(include.includes('customMetadata') ? { customMetadata: b.objects.get(key).meta?.customMetadata || {} } : {}) })),
      truncated: more, ...(more ? { cursor: page.at(-1) } : {}),
    };
  };
  return b;
}

const listing = (id, extra = {}) => ({
  id, name: NAME, author_id: AUTHOR, author_name: 'host', is_public: true, moderation: { status: 'approved', rating: 'all_ages' },
  r2_key: `universes/${id}/universe.pak`, format: 'pak', play_count: 7, version: 3, max_players: 4, space_names: ['Main'], published_at: '2026-10-01T00:00:00.000Z', ...extra,
});

/// Verify the author's address at `at` through the object's own operations.
async function verifyEmail(env, at) {
  const s = await accountCall(env, AUTHOR, 'email_verify_start', { now: at });
  assert.equal(s.ok, true, JSON.stringify(s));
  const c = await accountCall(env, AUTHOR, 'email_verify_confirm', { token: s.token.split('.')[1], now: at });
  assert.equal(c.ok, true, JSON.stringify(c));
}

async function environment({ setting = 'on', verifiedAgo = 31 * DAY, email = 'owner@example.com', sims = [SIM], extra = {} } = {}) {
  // ADJUSTED (fix): the removal for good is armed by its own setting, SPACE_HARD_DELETE; on here so the job's probes run it.
  const env = { ...fakeEnv(), SCENES: pagedBucket(), PAYOUTS: fakeKv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), LOGIN_ORIGINS: ORIGIN, DELETE_CODE_EMAIL: setting, SPACE_HARD_DELETE: 'on' };
  if (setting === null) delete env.DELETE_CODE_EMAIL;
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.EMAIL = { sent: [], async send(m) { this.sent.push(m); } };
  env.__EmailMessage = StubMessage;
  for (const [id, name] of [[AUTHOR, 'host'], [STRANGER, 'ada'], [ADMIN, 'staff']]) {
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: name, email: id === AUTHOR ? email : `${name}@example.com`, ...(id === ADMIN ? { role: 'admin' } : {}) }));
  }
  await accountState(env, AUTHOR);
  if (verifiedAgo !== null) await verifyEmail(env, Date.now() - verifiedAgo);
  for (const id of sims) {
    await env.SOCIAL.put(`sim:${id}`, JSON.stringify(listing(id, extra)));
    await env.SOCIAL.put(`sim-author:${AUTHOR}:${id}`, id);
    await env.SCENES.put(`universes/${id}/universe.pak`, new Uint8Array([1, 2, 3, 4]));
    await env.SCENES.put(`universes/${id}/spaces/Main.pak`, new Uint8Array([5, 6]));
    await env.SCENES.put(`universes/${id}/moderation/dossier.json`, '{"review":"kept"}');
    await env.SCENES.put(`universes/${id}/moderation/capture-0.png`, testPng());
    await env.SCENES.put(`thumbnails/${id}/thumb.png`, testPng(), { httpMetadata: { contentType: 'image/png' } });
  }
  await env.SOCIAL.put(`simCount:${AUTHOR}`, String(sims.length));
  return env;
}

const json = (data, status = 200, headers = {}) => new Response(JSON.stringify(data), { status, headers: { ...headers, 'Content-Type': 'application/json' } });
async function verifyJwt(token, secret) {
  const [h, p, s] = String(token).split('.');
  if (!h || !p || !s) return null;
  if (crypto.createHmac('sha256', secret).update(`${h}.${p}`).digest('base64url') !== s) return null;
  const payload = JSON.parse(Buffer.from(p, 'base64url').toString());
  return typeof payload.exp === 'number' && payload.exp >= Math.floor(Date.now() / 1000) ? payload : null;
}
async function verifyAuth(request, env) {
  const auth = request.headers.get('Authorization') || '';
  const claims = auth.startsWith('Bearer ') ? await verifyJwt(auth.slice(7), env.JWT_SECRET) : null;
  if (!claims?.sub) return null;
  const floor = Number(await env.USERS.get(`authfloor:${claims.sub}`)) || 0;
  return floor && !(claims.iat >= floor) ? null : claims.sub;
}
const requireAdmin = async (request, env) => { const id = await verifyAuth(request, env); return id === ADMIN ? id : null; };

/// Through the real Worker.
async function call(env, method, path, { who = 'author', body, headers = {} } = {}) {
  const h = { 'Content-Type': 'application/json', ...headers };
  if (who && tokens[who]) h.Authorization = `Bearer ${tokens[who]}`;
  const res = await worker.fetch(new Request(`${ORIGIN}${path}`, { method, headers: h, body: method === 'GET' || method === 'HEAD' ? undefined : body instanceof Uint8Array || typeof body === 'string' ? body : JSON.stringify(body || {}) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body: parsed, text, headers: res.headers };
}

/// Straight to the module, with a clock: for the fifteen minutes, the windows and the ninety days.
async function direct(env, method, path, { who = 'author', body, at = Date.now() } = {}) {
  const h = { 'Content-Type': 'application/json' };
  if (who && tokens[who]) h.Authorization = `Bearer ${tokens[who]}`;
  const request = new Request(`${ORIGIN}${path}`, { method, headers: h, body: method === 'GET' ? undefined : JSON.stringify(body || {}) });
  const res = await SD.handleSpaceDeleteRoute(request, new URL(request.url), env, ctx, {}, { json, verifyJwt, verifyAuth, requireAdmin, now: () => at });
  const text = await res.text();
  return { status: res.status, body: JSON.parse(text), text, headers: res.headers };
}

const deleteRequest = (env, id = SIM, opts = {}) => (opts.at !== undefined ? direct : call)(env, 'POST', `/api/account/spaces/${id}/delete-request`, { body: { typed_name: NAME }, ...opts });
const deleteConfirm = (env, request_id, code, id = SIM, opts = {}) => (opts.at !== undefined ? direct : call)(env, 'POST', `/api/account/spaces/${id}/delete-confirm`, { body: { request_id, typed_name: NAME, code }, ...opts });
const restoreRequest = (env, id = SIM, opts = {}) => (opts.at !== undefined ? direct : call)(env, 'POST', `/api/account/spaces/${id}/restore-request`, { body: { typed_name: NAME }, ...opts });
const restoreConfirm = (env, request_id, code, id = SIM, opts = {}) => (opts.at !== undefined ? direct : call)(env, 'POST', `/api/account/spaces/${id}/restore-confirm`, { body: { request_id, typed_name: NAME, code }, ...opts });
const lastCode = (env) => codeIn(env.EMAIL.sent.at(-1));

/// Request, read the code from the mail, confirm.
async function softDelete(env, id = SIM, opts = {}) {
  const r = await deleteRequest(env, id, opts);
  assert.equal(r.status, 200, r.text);
  const c = await deleteConfirm(env, r.body.request_id, lastCode(env), id, opts);
  assert.equal(c.status, 200, c.text);
  return { request: r, confirm: c };
}

const markOf = async (env, id = SIM) => { const o = await env.SCENES.get(`locks/deleted/${id}.json`); return o ? JSON.parse(await o.text()) : null; };
const inGallery = async (env, id = SIM) => ((await call(env, 'GET', '/api/simulations', { who: null })).body?.simulations || []).some((s) => s.id === id);
const objectState = (env) => JSON.parse(JSON.stringify(env.ACCOUNT_AUTH.instances.get(AUTHOR) ? [...env.ACCOUNT_AUTH.instances.get(AUTHOR).storage.map] : []));
const pendingOf = (env, id = SIM) => env.ACCOUNT_AUTH.instances.get(AUTHOR)?.storage.map.get('state')?.space_delete?.pending?.[id] || null;


const HOLDS = {
  legal_lock: async (env, id) => { await env.SCENES.put(`locks/legal/${id}`, '{"takedown":true}', { customMetadata: { kind: 'serve_block' } }); },
  case_legal_hold: async (env, id) => { await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status: 'approved', legal_hold: { category: 'csam', preserve_until: new Date(Date.now() + 300 * DAY).toISOString() } })); },
  quarantine_marker: async (env, id) => { const s = JSON.parse(await env.SOCIAL.get(`sim:${id}`)); s.moderation = { status: 'quarantined' }; await env.SOCIAL.put(`sim:${id}`, JSON.stringify(s)); },
  quarantine_case: async (env, id) => { await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status: 'quarantined' })); },
  legal_lane_hold: async (env, id) => { await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status: 'held', decision: { lane: 'legal' } })); },
  report_debt: async (env, id) => { await env.SOCIAL.put(`legalowed:moderation:${id}`, new Date().toISOString()); },
  report_stuck: async (env, id) => { await env.SOCIAL.put(`legalstuck:moderation:${id}:ncmec`, '{}'); },
  failed_seal: async (env, id) => { await env.SOCIAL.put(`vault-pending:moderation:${id}:pak`, '{}'); },
  evidence_in_place: async (env, id) => { await env.SOCIAL.put(`caseref-of:moderation:${id}`, REF); await env.SCENES.put(`evidence/${REF}/pak.json`, JSON.stringify({ copied: false })); },
  evidence_never_delete: async (env, id) => { await env.SOCIAL.put(`caseref-of:moderation:${id}`, REF); await env.SCENES.put(`evidence/${REF}/pak.json`, JSON.stringify({ copied: true, never_delete: true })); },
  takedown: async (env, id) => { await env.SOCIAL.put(`tdlisting:${id}:td-1`, '1'); await env.SOCIAL.put('tdcase:td-1', JSON.stringify({ id: 'td-1', status: 'received' })); },
  takedown_seal: async (env, id) => { await env.SOCIAL.put(`tdlisting:${id}:td-2`, '1'); await env.SOCIAL.put('tdcase:td-2', JSON.stringify({ id: 'td-2', status: 'closed' })); await env.SOCIAL.put('vault-pending:takedown:td-2:pak', '{}'); },
  open_review: async (env, id) => { await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status: 'classifying' })); },
  appeal_pending: async (env, id) => { await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status: 'rejected', appeal: { status: 'pending' } })); },
  unreviewed_upload: async (env, id) => { const s = JSON.parse(await env.SOCIAL.get(`sim:${id}`)); s.moderation = { status: 'pending' }; await env.SOCIAL.put(`sim:${id}`, JSON.stringify(s)); },
  workspace_hold: async (env, id) => { await env.SOCIAL.put(`wsq:hold:${id}`, '{}'); },
  preserved_after_release: async (env, id) => { await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status: 'approved', legal_release: { preserve_until: new Date(Date.now() + 200 * DAY).toISOString(), released_at: new Date().toISOString() } })); },
  never_delete_after_release: async (env, id) => { await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, status: 'approved', legal_release: { never_delete: true, preserve_until: null } })); },
  staff_hold: async (env, id) => { await env.SCENES.put(`locks/space-delete-hold/${id}.json`, JSON.stringify({ by: ADMIN, rationale: 'a person is looking at this listing' })); },
};

// =============================================================================
// SECURITY PROBES (independent review, 2026-11-12). Each probe asserts what the
// code SHOULD do; on the code as reviewed, each one FAILS. Helpers above are
// copied verbatim from tests/space_delete.test.mjs.
// =============================================================================

const { blockedHash } = await workerModule('takedown.mjs');
const MOD = await workerModule('moderation.mjs');

const sha256 = (bytes) => crypto.createHash('sha256').update(Buffer.from(bytes)).digest('hex');

/// A soft-deleted listing as the job finds it, seeded directly (the request flow allows five a day per account).
async function seedDeleted(env, id, softAt) {
  const hardAfter = softAt + 90 * DAY;
  const mark = {
    v: 1, sim_id: id, author_id: AUTHOR, name: NAME, state: 'soft', request_id: `sdr_${'A'.repeat(24)}`, factor: 'email',
    soft_deleted_at: new Date(softAt).toISOString(), hard_delete_after: new Date(hardAfter).toISOString(), was_public: true, was_archived: false,
    deferred: null, progress: null,
  };
  await env.SCENES.put(`locks/deleted/${id}.json`, JSON.stringify(mark), {
    customMetadata: { state: 'soft', author: AUTHOR, soft_deleted_at: mark.soft_deleted_at, hard_delete_after: mark.hard_delete_after, request_id: mark.request_id },
  });
  await env.SOCIAL.put(`spacedel:due:${new Date(hardAfter).toISOString().slice(0, 13)}:${id}`, JSON.stringify({ sim_id: id }));
}

// ── M1 ───────────────────────────────────────────────────────────────────────
test('PROBE M1: listings held for good starve every later removal; the job spends its whole budget on them each hour', async () => {
  const T = Date.now();
  const held = Array.from({ length: 30 }, (_, i) => `dddddddd-0000-4000-8000-${String(i).padStart(12, '0')}`);
  const env = await environment({ sims: [...held, SIM] });
  for (const id of held) {
    await seedDeleted(env, id, T - 200 * DAY);
    await HOLDS.never_delete_after_release(env, id); // possible CSAM once, released: never removed (D7)
  }
  await seedDeleted(env, SIM, T - 95 * DAY); // an ordinary deletion, five days past due, nothing holds it
  const runs = [];
  for (let hour = 0; hour < 3; hour++) runs.push(await SD.sweepSpaceDeletions(env, {}, { now: T + hour * HOUR }));
  const mark = await markOf(env, SIM);
  assert.equal(mark.state, 'hard_deleted', `three hourly runs never reached the ordinary listing (state ${mark.state}); runs: ${JSON.stringify(runs.map((r) => ({ due: r.due, deferred: r.deferred, budget_spent: r.budget_spent })))}`);
});

// ── M2 ───────────────────────────────────────────────────────────────────────
test('PROBE M2: with DELETE_CODE_EMAIL switched off nobody can restore, yet the job still removes for good', async () => {
  const T = Date.now();
  const env = await environment();
  await softDelete(env, SIM, { at: T });
  env.DELETE_CODE_EMAIL = 'off'; // the documented response to a mail problem
  const r = await restoreRequest(env, SIM, { at: T + 80 * DAY });
  assert.equal(r.status, 503, 'the owner cannot restore while it is off');
  await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY });
  assert.ok(env.SCENES.objects.has(`universes/${SIM}/universe.pak`), 'removed for good during a period when the promised restore was impossible');
});

// ── M3 ───────────────────────────────────────────────────────────────────────
test('PROBE M3: evidence kept in place under a TAKE IT DOWN reference (never_delete, copied:false) does not hold the removal', async () => {
  const T = Date.now();
  const env = await environment();
  await softDelete(env, SIM, { at: T });
  // A possible-CSAM takedown named the listing; it is closed now. Its seal (evidence_vault sealEvidence, caseKind 'takedown')
  // kept the package in place (over 512 MB, or a world's chunks). The moderation case no longer carries never_delete (a later,
  // unrelated hold and release rewrote legal_release, moderation.mjs:7233 and :7280).
  const TDREF = 'EC-TTTT-TTTT-TTTT-TTTT-TTTT';
  await env.SOCIAL.put(`tdlisting:${SIM}:td-9`, '1');
  await env.SOCIAL.put('tdcase:td-9', JSON.stringify({ id: 'td-9', status: 'closed' }));
  await env.SOCIAL.put('caseref-of:takedown:td-9', TDREF);
  await env.SCENES.put(`evidence/${TDREF}/aaaaaaaa-package.json`, JSON.stringify({
    v: 1, case_ref: TDREF, case_kind: 'takedown', case_id: 'td-9', item: 'aaaaaaaa-package', category: 'csam', content: 'r2_object', copied: false, never_delete: true,
  }));
  await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY });
  assert.ok(env.SCENES.objects.has(`universes/${SIM}/universe.pak`), 'the bytes a never_delete evidence item points at were deleted');
});

// ── M4 ───────────────────────────────────────────────────────────────────────
test('PROBE M4: after the removal a TAKE IT DOWN naming the listing finds nothing and blocks none of its files; the tombstone hashes are never read', async () => {
  const T = Date.now();
  const PAK_SHA = sha256(new Uint8Array([1, 2, 3, 4]));
  const env = await environment({ extra: { pak_sha256: PAK_SHA } });
  await softDelete(env, SIM, { at: T });
  await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY });
  assert.equal((await markOf(env)).state, 'hard_deleted');
  assert.equal(JSON.parse(await env.SOCIAL.get(`simgone:${SIM}`)).pak_sha256, PAK_SHA, 'the tombstone has the hash');
  const r = await call(env, 'POST', '/api/takedown/requests', {
    who: null, headers: { 'CF-Connecting-IP': '203.0.113.7' },
    body: {
      requester_name: 'Rowan Quillfeather', signature: 'Rowan Quillfeather', relationship: 'self', contact_email: 'rowan.quill@example.org',
      locations: [`https://eustress.dev/simulation/${SIM}`], description: 'A private photo of me used in this Space, shared without my consent.', good_faith: true,
    },
  });
  assert.equal(r.status, 201, r.text);
  assert.ok(await blockedHash(env, [PAK_SHA]), `the deleted listing's package is not on the blocklist; location result: ${JSON.stringify(r.body.locations)}`);
});

// ── M5 ───────────────────────────────────────────────────────────────────────
// McKale's decision (2026-11-13, option A): a retention-only flag left by a released hold still refuses the deletion, but the
// author is told "can't be deleted right now" (code cannot_delete_now), never "being reviewed". Live holds keep the in_review copy.
test('PROBE M5 (decided, option A): a retention-only flag left by a RELEASED hold refuses the deletion without saying a review or a hold exists', async () => {
  const env = await environment();
  await HOLDS.preserved_after_release(env, SIM);
  // The listing serves and takes writes as usual: nothing tells the author that a hold was ever placed.
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`, { who: 'stranger' })).status, 200);
  assert.equal((await call(env, 'POST', `/api/simulations/${SIM}/archive`, { body: {} })).status, 200);
  // The deletion status read, which the author may poll freely, says "being reviewed".
  const status = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`);
  assert.deepEqual(status.body.blocked, NEUTRAL, `the status read must be the neutral copy: ${JSON.stringify(status.body.blocked)}`);
  assert.notDeepEqual(status.body.blocked, IN_REVIEW);
  // The refusal itself, on the request and again on the confirm path, carries the same words and opens and mails nothing.
  const r = await deleteRequest(env);
  assert.equal(r.status, 409, r.text);
  assert.deepEqual(r.body, NEUTRAL);
  assert.equal(env.EMAIL.sent.length, 0, 'nothing mailed');
});

test('M5: the neutral copy never names a hold, a review, a report or a legal reason, and a live hold still answers in_review', async () => {
  const words = JSON.stringify(NEUTRAL).toLowerCase();
  for (const banned of ['hold', 'review', 'report', 'legal', 'law', 'retention', 'preserv', 'evidence', 'takedown', 'take it down', 'quarantine', 'investigat']) {
    assert.ok(!words.includes(banned), `the neutral copy names "${banned}": ${words}`);
  }
  // Every retention-only marker, alone, answers the neutral copy on the request and on the status read.
  for (const kind of ['preserved_after_release', 'never_delete_after_release', 'report_debt', 'report_stuck', 'failed_seal', 'evidence_in_place']) {
    const env = await environment();
    await HOLDS[kind](env, SIM);
    assert.deepEqual((await deleteRequest(env)).body, NEUTRAL, kind);
    assert.deepEqual((await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`)).body.blocked, NEUTRAL, kind);
  }
  // A live hold, even with a retention marker beside it, keeps the in_review copy.
  for (const kind of ['legal_lock', 'quarantine_case', 'open_review', 'takedown', 'staff_hold', 'workspace_hold']) {
    const env = await environment();
    await HOLDS.preserved_after_release(env, SIM);
    await HOLDS[kind](env, SIM);
    if (kind === 'quarantine_case' || kind === 'open_review') await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, status: kind === 'open_review' ? 'classifying' : 'quarantined', legal_release: { preserve_until: new Date(Date.now() + 200 * DAY).toISOString() } }));
    assert.deepEqual((await deleteRequest(env)).body, IN_REVIEW, kind);
    assert.deepEqual((await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`)).body.blocked, IN_REVIEW, kind);
  }
});

test('DELETE_MAIL_FROM: the code mail and the deleted and restored notices come from support@eustress.dev as Eustress Support, or from the setting', async () => {
  for (const [setting, expected] of [[undefined, 'support@eustress.dev'], ['help@eustress.dev', 'help@eustress.dev']]) {
    const env = await environment();
    if (setting) env.DELETE_MAIL_FROM = setting;
    const r = await deleteRequest(env);
    assert.equal(r.status, 200, r.text);
    const c = await deleteConfirm(env, r.body.request_id, lastCode(env));
    assert.equal(c.status, 200, c.text);
    assert.ok(env.EMAIL.sent.length >= 2, 'the code mail and the deleted notice were sent');
    for (const m of env.EMAIL.sent) {
      assert.equal(m.from, expected);
      assert.match(m.raw, new RegExp(`^From: Eustress Support <${expected}>`, 'm'));
    }
  }
});

// ── L1 ───────────────────────────────────────────────────────────────────────
test('PROBE L1: the job may run 10 minutes on one listing under a 6-minute lease it never extends; it keeps deleting after another holder took the lease', async () => {
  const T = Date.now();
  const env = await environment();
  await env.SCENES.put(`covers/${SIM}/cover.png`, testPng());
  await softDelete(env, SIM, { at: T });
  const realNow = Date.now;
  let skew = 0;
  let other = null;
  env.SCENES.beforeDelete = async (keys) => {
    if (!Array.isArray(keys) || other) return;
    skew = 7 * MIN; // the first page took longer than lease_seconds (360)
    other = await MOD.acquireLease(env, SIM, MOD.thresholdsFrom(env), 'run');
  };
  Date.now = () => realNow() + skew;
  try {
    await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY });
  } finally {
    Date.now = realNow;
  }
  assert.ok(other, 'a review run took the expired lease mid-job');
  assert.ok(env.SCENES.objects.has(`covers/${SIM}/cover.png`), 'the job went on deleting while another run held the lease');
});

// ── L2 ───────────────────────────────────────────────────────────────────────
test('PROBE L2: /api/ledger/spend tells a soft-deleted listing from a missing one (and would stamp its name on a receipt)', async () => {
  const env = await environment();
  await softDelete(env);
  const spend = (id) => call(env, 'POST', '/api/ledger/spend', { who: 'stranger', body: { amount: 1, purpose: 'probe', simulation_id: id } });
  const deleted = await spend(SIM);
  const missing = await spend(SIM3);
  assert.equal(deleted.status, missing.status, `deleted ${deleted.status} ${deleted.text} vs missing ${missing.status} ${missing.text}`);
});

// ── L3 ───────────────────────────────────────────────────────────────────────
test('PROBE L3: a create-if-absent conflict deletes the WINNING deletion\'s due key when both fall in the same hour', async () => {
  const env = await environment();
  const at = Date.now();
  const other = { v: 1, sim_id: SIM, author_id: AUTHOR, name: NAME, state: 'soft', request_id: 'sdr_OTHEROTHEROTHEROTHEROTH', soft_deleted_at: new Date(at).toISOString(), hard_delete_after: new Date(at + 90 * DAY).toISOString() };
  const otherDue = `spacedel:due:${other.hard_delete_after.slice(0, 13)}:${SIM}`;
  const r = await deleteRequest(env);
  assert.equal(r.status, 200, r.text);
  await env.SCENES.put(`locks/deleted/${SIM}.json`, JSON.stringify(other), { customMetadata: { state: 'soft', author: AUTHOR, soft_deleted_at: other.soft_deleted_at, hard_delete_after: other.hard_delete_after, request_id: other.request_id } });
  await env.SOCIAL.put(otherDue, JSON.stringify({ sim_id: SIM, request_id: other.request_id }));
  const realHead = env.SCENES.head;
  env.SCENES.head = async (k) => (k === `locks/deleted/${SIM}.json` ? null : realHead(k));
  const c = await deleteConfirm(env, r.body.request_id, lastCode(env));
  env.SCENES.head = realHead;
  assert.equal(c.body.code, 'already_deleted');
  assert.ok(await env.SOCIAL.get(otherDue), 'the mark that won has lost its due key, so the job never removes it');
});

// ── L4 ───────────────────────────────────────────────────────────────────────
test('PROBE L4: a listing name of the author\'s (or a session thief\'s) choosing goes verbatim into the Security Notice that carries the code', async () => {
  const lure = 'Account locked visit https://eustress-verify.example/login';
  const env = await environment({ extra: { name: lure } });
  const r = await call(env, 'POST', `/api/account/spaces/${SIM}/delete-request`, { body: { typed_name: lure } });
  assert.equal(r.status, 200, r.text);
  assert.ok(!bodyOf(env.EMAIL.sent.at(-1)).includes('https://eustress-verify.example'), 'an address of the author\'s choosing is in the code mail');
});

// ── L5 ───────────────────────────────────────────────────────────────────────
test('PROBE L5: the hourly job\'s outcome (deferred, failed, budget spent) never reaches the night record', async () => {
  const env = await environment();
  const waits = [];
  await worker.scheduled({ scheduledTime: Date.parse('2026-11-13T00:00:00.000Z') }, env, { waitUntil: (p) => waits.push(p), passThroughOnException() {} });
  await Promise.all(waits);
  const night = JSON.parse((await env.PAYOUTS.get('cronrun:2026-11-13')) || 'null');
  assert.ok(night, 'the night record was written');
  assert.ok(Object.keys(night).some((k) => k.includes('space')), `no space deletion counts in the night record: ${Object.keys(night).join(', ')}`);
});

// =============================================================================
// THE FIXES' OWN TESTS (applied 2026-11-12): each checks one fix beyond its probe above.
// =============================================================================

const TAKEDOWN_BODY = (locations) => ({
  requester_name: 'Rowan Quillfeather', signature: 'Rowan Quillfeather', relationship: 'self', contact_email: 'rowan.quill@example.org',
  locations, description: 'A private photo of me used in this Space, shared without my consent.', good_faith: true,
});

// ── The switch ───────────────────────────────────────────────────────────────
test('FIX SWITCH: the hourly job reads and removes nothing unless SPACE_HARD_DELETE is exactly "on", and both environments ship it "off"', async () => {
  const T = Date.now();
  for (const value of [undefined, 'off', 'On', 'true', ' on']) {
    const env = await environment();
    await softDelete(env, SIM, { at: T });
    if (value === undefined) delete env.SPACE_HARD_DELETE; else env.SPACE_HARD_DELETE = value;
    const r2Before = JSON.stringify(env.SCENES.calls);
    const socialList = env.SOCIAL.list;
    let listed = 0;
    env.SOCIAL.list = async (...a) => { listed++; return socialList.apply(env.SOCIAL, a); };
    const job = await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY });
    env.SOCIAL.list = socialList;
    assert.equal(job.switched_off, true, String(value));
    assert.equal(job.due, 0, String(value));
    assert.equal(listed, 0, `${value}: the due index is not even listed`);
    assert.equal(JSON.stringify(env.SCENES.calls), r2Before, `${value}: no R2 call`);
    assert.equal((await markOf(env)).state, 'soft', String(value));
    assert.ok(env.SCENES.objects.has(`universes/${SIM}/universe.pak`), String(value));
  }
  const toml = fs.readFileSync(path.resolve(import.meta.dirname, '..', 'wrangler.toml'), 'utf8');
  assert.equal((toml.match(/\r\n/g) || []).length, (toml.match(/\n/g) || []).length, 'wrangler.toml stays CRLF');
  const text = toml.split('\r\n').join('\n');
  const block = (header) => {
    const at = text.indexOf(`\n${header}\n`);
    assert.ok(at >= 0, header);
    const end = text.indexOf('\n[', at + header.length + 1);
    return text.slice(at, end < 0 ? undefined : end);
  };
  for (const header of ['[vars]', '[env.staging.vars]']) assert.match(block(header), /\nSPACE_HARD_DELETE = "off"\n/, header);
});

// ── M1 ───────────────────────────────────────────────────────────────────────
test('FIX M1: a held listing is filed again at the next hour, so it no longer heads the due index, and the run counts it', async () => {
  const T = Date.now();
  const env = await environment();
  await seedDeleted(env, SIM, T - 100 * DAY);
  await HOLDS.staff_hold(env, SIM);
  const dueKeys = () => [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('spacedel:due:'));
  const before = dueKeys();
  const job = await SD.sweepSpaceDeletions(env, {}, { now: T });
  assert.equal(job.deferred.staff_hold, 1, JSON.stringify(job));
  assert.equal(job.refiled, 1, JSON.stringify(job));
  assert.deepEqual(dueKeys(), [`spacedel:due:${new Date(T + HOUR).toISOString().slice(0, 13)}:${SIM}`], `was ${before}`);
  // Still checked every hour: released, it goes at the next run.
  await env.SCENES.delete(`locks/space-delete-hold/${SIM}.json`);
  const next = await SD.sweepSpaceDeletions(env, {}, { now: T + HOUR });
  assert.equal(next.hard_deleted, 1, JSON.stringify(next));
});

// ── M2 ───────────────────────────────────────────────────────────────────────
test('FIX M2: with the code mail off a removal already begun goes no further either; the reason is on the mark, and it resumes once back on', async () => {
  const T = Date.now();
  const env = await environment();
  for (let i = 0; i < 50; i++) await env.SCENES.put(`universes/${SIM}/chunks/${String(i).padStart(64, '0')}.echk`, new Uint8Array([i]));
  await softDelete(env, SIM, { at: T });
  await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY, budget: 40 });
  assert.equal((await markOf(env)).state, 'hard_deleting', 'removal has begun');
  const left = () => [...env.SCENES.objects.keys()].filter((k) => k.startsWith(`universes/${SIM}/`)).sort().join('\n');
  const before = left();
  env.DELETE_CODE_EMAIL = 'off';
  const job = await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY + HOUR });
  assert.equal(job.deferred.restore_unavailable, 1, JSON.stringify(job));
  assert.equal(left(), before, 'nothing more was removed');
  assert.ok((await markOf(env)).deferred.reasons.includes('restore_unavailable'));
  env.DELETE_CODE_EMAIL = 'on';
  for (let hour = 2; hour < 10 && (await markOf(env)).state !== 'hard_deleted'; hour++) await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY + hour * HOUR });
  assert.equal((await markOf(env)).state, 'hard_deleted');
});

// ── M4 and L6 ────────────────────────────────────────────────────────────────
test('FIX M4 + L6: the tombstone holds each removed file\'s own SHA-256, and a TAKE IT DOWN after the removal blocks the package, the thumbnail and the kept review capture', async () => {
  const T = Date.now();
  const env = await environment(); // the listing carries no pak_sha256: every hash below comes from the bytes
  const pak = sha256(new Uint8Array([1, 2, 3, 4]));
  const thumb = sha256(env.SCENES.objects.get(`thumbnails/${SIM}/thumb.png`).body);
  const capture = sha256(env.SCENES.objects.get(`universes/${SIM}/moderation/capture-0.png`).body);
  await softDelete(env, SIM, { at: T });
  await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY });
  assert.equal((await markOf(env)).state, 'hard_deleted');
  const tomb = JSON.parse(await env.SOCIAL.get(`simgone:${SIM}`));
  assert.equal(tomb.pak_sha256, null, 'the listing had no hash of its own');
  const files = (await Promise.all([...env.SCENES.objects.keys()].filter((k) => k.startsWith(`tombstones/${SIM}/`)).map(async (k) => JSON.parse(await (await env.SCENES.get(k)).text()).files))).flat();
  assert.equal(files.find((f) => f.key === `universes/${SIM}/universe.pak`)?.sha256, pak, JSON.stringify(files));
  assert.equal(files.find((f) => f.key === `universes/${SIM}/spaces/Main.pak`)?.sha256, sha256(new Uint8Array([5, 6])));
  assert.equal(files.find((f) => f.key === `thumbnails/${SIM}/thumb.png`)?.sha256, thumb);
  assert.ok(files.every((f) => /^[0-9a-f]{64}$/.test(f.sha256) && f.sha256_from === 'read'), JSON.stringify(files));
  assert.ok(tomb.file_hashes.some((h) => h.sha256 === pak && h.kind === 'package'), JSON.stringify(tomb.file_hashes));
  for (const h of [pak, thumb, capture]) assert.equal(await blockedHash(env, [h]), null, 'nothing is blocked before a request');
  const r = await call(env, 'POST', '/api/takedown/requests', {
    who: null, headers: { 'CF-Connecting-IP': '203.0.113.8' },
    body: TAKEDOWN_BODY([`https://eustress.dev/simulation/${SIM}`, `https://eustress.dev/simulation/${SIM3}`]),
  });
  assert.equal(r.status, 201, r.text);
  assert.deepEqual(r.body.locations.map((l) => l.result), ['removed', 'not_found'], 'the removed listing reads as removed; one that never existed is still not found');
  for (const [what, h] of [['package', pak], ['thumbnail', thumb], ['review capture', capture]]) assert.ok(await blockedHash(env, [h]), `the ${what} is on the blocklist`);
  assert.equal(await env.SCENES.head(`locks/legal/${SIM}`), null, 'no lock is written for a listing that is gone');
});

// ── L4 ───────────────────────────────────────────────────────────────────────
test('FIX L4: the deleted and restored notices keep the name, cut short, with no address a mail client would link', async () => {
  const lure = `Account locked visit https://eustress-verify.example/login ${'x'.repeat(200)}`;
  for (const kind of ['space_deleted', 'space_restored']) {
    const n = SD.renderSpaceNotice(kind, { name: lure, at: Date.now(), hardDeleteAfter: Date.now() + 90 * DAY });
    for (const part of [n.text, n.html]) {
      assert.ok(!part.includes('https://eustress-verify.example'), `${kind}: the address is linkable`);
      assert.ok(!part.includes('x'.repeat(100)), `${kind}: the name is not cut short`);
      assert.ok(part.includes('Account locked visit'), `${kind}: the name is still there`);
    }
  }
});
