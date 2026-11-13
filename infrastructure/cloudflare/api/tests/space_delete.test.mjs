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
// A hold that is only a retention marker left behind refuses a deletion with this copy instead (review M5, option A).
const CANNOT_DELETE_NOW = { error: "This Space can't be deleted right now. Contact support@eustress.dev.", code: 'cannot_delete_now' };
const RETENTION_ONLY_KINDS = ['report_debt', 'report_stuck', 'failed_seal', 'evidence_in_place', 'evidence_never_delete', 'takedown_seal', 'preserved_after_release', 'never_delete_after_release'];
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
  // ADJUSTED (fix): the removal for good is armed by its own setting (SPACE_HARD_DELETE), on here so the job's tests run it.
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

// ── The name ─────────────────────────────────────────────────────────────────

test('the typed name must match exactly: case, inner spaces and an empty name are refused, and nothing is sent', async () => {
  const env = await environment();
  for (const typed of ['the super station', 'The  Super Station', 'The Super Statio', '', '   ', null, 42]) {
    const r = await call(env, 'POST', `/api/account/spaces/${SIM}/delete-request`, { body: { typed_name: typed } });
    assert.equal(r.status, 400, `${JSON.stringify(typed)}: ${r.text}`);
    assert.equal(r.body.code, 'name_mismatch');
  }
  assert.equal(env.EMAIL.sent.length, 0, 'no code was mailed for a wrong name');
  assert.equal(pendingOf(env), null, 'and nothing was opened in the account object');
  // Surrounding spaces are trimmed; the name itself is exact.
  const ok = await call(env, 'POST', `/api/account/spaces/${SIM}/delete-request`, { body: { typed_name: `  ${NAME}\n` } });
  assert.equal(ok.status, 200, ok.text);
  // And at confirm the name is asked again.
  const wrong = await call(env, 'POST', `/api/account/spaces/${SIM}/delete-confirm`, { body: { request_id: ok.body.request_id, typed_name: 'Super Station', code: lastCode(env) } });
  assert.equal(wrong.status, 400);
  assert.equal(wrong.body.code, 'name_mismatch');
  assert.equal(await markOf(env), null);
});

test('a listing renamed between the request and the confirm is refused as renamed', async () => {
  const env = await environment();
  const r = await deleteRequest(env);
  const code = lastCode(env);
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { name: 'Renamed Station' })));
  const c = await call(env, 'POST', `/api/account/spaces/${SIM}/delete-confirm`, { body: { request_id: r.body.request_id, typed_name: 'Renamed Station', code } });
  assert.equal(c.status, 409, c.text);
  assert.equal(c.body.code, 'name_changed');
  assert.equal(await markOf(env), null);
});

test('a stranger and a missing listing get the same 404, and a request without a session is 401', async () => {
  const env = await environment();
  const stranger = await call(env, 'POST', `/api/account/spaces/${SIM}/delete-request`, { who: 'stranger', body: { typed_name: NAME } });
  const missing = await call(env, 'POST', `/api/account/spaces/${SIM3}/delete-request`, { body: { typed_name: NAME } });
  assert.equal(stranger.status, 404);
  assert.equal(missing.status, 404);
  assert.equal(stranger.text, missing.text, 'not told apart');
  assert.equal((await call(env, 'POST', `/api/account/spaces/${SIM}/delete-request`, { who: null, body: { typed_name: NAME } })).status, 401);
  const statusStranger = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`, { who: 'stranger' });
  assert.equal(statusStranger.status, 404);
});

// ── The setting ──────────────────────────────────────────────────────────────

test('with DELETE_CODE_EMAIL anything but exactly "on", every route answers 503 delete_unavailable and nothing is mailed', async () => {
  for (const setting of [null, 'off', 'ON', 'true', 'on ', '1', '']) {
    const env = await environment({ setting });
    for (const step of ['delete-request', 'delete-confirm', 'restore-request', 'restore-confirm']) {
      const r = await call(env, 'POST', `/api/account/spaces/${SIM}/${step}`, { body: { typed_name: NAME, request_id: 'sdr_AAAAAAAAAAAAAAAAAAAAAAAA', code: '123456' } });
      assert.equal(r.status, 503, `${setting} ${step}: ${r.text}`);
      assert.deepEqual(r.body, { error: 'Deleting or restoring a published Space is not available yet.', code: 'delete_unavailable' });
    }
    assert.equal(env.EMAIL.sent.length, 0);
    const status = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`);
    assert.equal(status.status, 200);
    assert.equal(status.body.blocked.code, 'delete_unavailable');
    assert.equal(status.body.can_delete, false);
  }
});

// ── The address ──────────────────────────────────────────────────────────────

test('the code goes only to the verified address, and only once it is thirty days verified', async () => {
  const unverified = await environment({ verifiedAgo: null });
  const r1 = await deleteRequest(unverified);
  assert.equal(r1.status, 409, r1.text);
  assert.equal(r1.body.code, 'no_verified_email');
  assert.equal(unverified.EMAIL.sent.length, 0, 'never to an address on file that was not verified');

  const young = await environment({ verifiedAgo: 29 * DAY });
  const r2 = await deleteRequest(young);
  assert.equal(r2.status, 409);
  assert.equal(r2.body.code, 'no_verified_email');
  assert.equal(young.EMAIL.sent.length, 0, 'never to an address verified less than thirty days ago');
  assert.equal(pendingOf(young), null, 'and the refused request opened nothing and spent no allowance');

  // A pending address change names a new address: the code still goes to the verified one alone.
  const env = await environment();
  const state = env.ACCOUNT_AUTH.instances.get(AUTHOR).storage.map.get('state');
  state.email.pending = { address: 'thief@example.net', requested_at: new Date().toISOString(), effective_at: Date.now() + 3 * DAY, cancel_digest: 'f'.repeat(64) };
  env.ACCOUNT_AUTH.instances.get(AUTHOR).storage.map.set('state', state);
  const r3 = await deleteRequest(env);
  assert.equal(r3.status, 200, r3.text);
  assert.equal(env.EMAIL.sent.length, 1);
  assert.equal(env.EMAIL.sent[0].to, 'owner@example.com');
  assert.equal(r3.body.sent_to, 'o***@example.com');
  assert.ok(!env.EMAIL.sent.some((m) => m.to === 'thief@example.net'));
  assert.equal(subjectOf(env.EMAIL.sent[0]), 'Your Eustress confirmation code', 'the subject names neither the Space nor the code');
});

// ── The code: expiry, attempts, once ─────────────────────────────────────────

test('a code works for fifteen minutes: at 14:59 it deletes, at 15:00 it has expired', async () => {
  const T = Date.now();
  const late = await environment();
  const r = await deleteRequest(late, SIM, { at: T });
  const expired = await deleteConfirm(late, r.body.request_id, lastCode(late), SIM, { at: T + 15 * MIN });
  assert.equal(expired.status, 410, expired.text);
  assert.equal(expired.body.code, 'request_expired');
  assert.equal(await markOf(late), null);

  const inTime = await environment();
  const r2 = await deleteRequest(inTime, SIM, { at: T });
  const ok = await deleteConfirm(inTime, r2.body.request_id, lastCode(inTime), SIM, { at: T + 15 * MIN - 1000 });
  assert.equal(ok.status, 200, ok.text);
  assert.equal((await markOf(inTime)).state, 'soft');
});

test('five wrong codes lock the request; the right code afterwards is refused and nothing is deleted', async () => {
  const env = await environment();
  const r = await deleteRequest(env);
  const code = lastCode(env);
  const wrong = String((Number(code) + 1) % 1_000_000).padStart(6, '0');
  for (let i = 1; i <= 4; i++) {
    const c = await deleteConfirm(env, r.body.request_id, wrong);
    assert.equal(c.status, 401, c.text);
    assert.equal(c.body.code, 'code_invalid');
    assert.equal(c.body.attempts_left, 5 - i);
  }
  const fifth = await deleteConfirm(env, r.body.request_id, wrong);
  assert.equal(fifth.status, 401);
  assert.equal(fifth.body.attempts_left, 0);
  assert.equal(fifth.body.locked, true);
  const right = await deleteConfirm(env, r.body.request_id, code);
  assert.equal(right.status, 409, right.text);
  assert.equal(right.body.code, 'request_locked');
  assert.equal(await markOf(env), null, 'nothing was deleted');
  assert.equal(pendingOf(env).status, 'locked');
});

test('the attempts are counted in the account object, so parallel confirms cannot get more than five tries', async () => {
  const env = await environment();
  const r = await deleteRequest(env);
  const code = lastCode(env);
  const wrong = String((Number(code) + 7) % 1_000_000).padStart(6, '0');
  // Through the Worker, twelve at once: the moderation lease lets one through at a time (the rest read as busy), and none
  // gets more than the five tries.
  const answers = await Promise.all(Array.from({ length: 12 }, () => deleteConfirm(env, r.body.request_id, wrong)));
  assert.ok(answers.every((a) => ['code_invalid', 'in_review', 'request_locked'].includes(a.body.code)), JSON.stringify(answers.map((a) => a.body.code)));
  assert.ok(answers.filter((a) => a.body.code === 'code_invalid').length <= 5);
  // In the object itself, twelve at once with a wrong digest: exactly five are judged, the rest find the request locked.
  const r2 = await deleteRequest(env);
  assert.equal(r2.status, 200, r2.text);
  const session = crypto.createHash('sha256').update(`eustress-session:${tokens.author}`).digest('hex');
  const pending = pendingOf(env);
  const at12 = await Promise.all(Array.from({ length: 12 }, () => accountCall(env, AUTHOR, 'space_delete_confirm', {
    sim_id: SIM, action: 'delete', request_id: r2.body.request_id, code_digest: 'f'.repeat(64), name_sha256: pending.name_sha256, session_sha256: session,
  })));
  assert.equal(at12.filter((a) => a.code === 'code_invalid').length, 5, 'exactly five wrong answers were judged');
  assert.equal(at12.filter((a) => a.code === 'request_locked').length, 7);
  assert.equal((await deleteConfirm(env, r2.body.request_id, lastCode(env))).body.code, 'request_locked');
  assert.equal(await markOf(env), null);
});

test('a code works once: the same confirm again is a replay that changes nothing, another code is refused, and a used code opens no new request', async () => {
  const env = await environment();
  const { request, confirm } = await softDelete(env);
  const code = codeIn(env.EMAIL.sent.find((m) => subjectOf(m) === 'Your Eustress confirmation code'));
  const markBefore = await markOf(env);
  const auditBefore = [...env.AUDIT_LOG.store.keys()].length;
  const mailBefore = env.EMAIL.sent.length;
  const again = await deleteConfirm(env, request.body.request_id, code);
  assert.equal(again.status, 200, again.text);
  assert.equal(again.body.replay, true);
  assert.equal(again.body.hard_delete_after, confirm.body.hard_delete_after);
  assert.deepEqual(await markOf(env), markBefore, 'the mark is unchanged');
  assert.equal([...env.AUDIT_LOG.store.keys()].length, auditBefore, 'no second audit record');
  assert.equal(env.EMAIL.sent.length, mailBefore, 'no second notice');
  // The same request with another code: used.
  const other = await deleteConfirm(env, request.body.request_id, String((Number(code) + 1) % 1_000_000).padStart(6, '0'));
  assert.equal(other.status, 409);
  assert.equal(other.body.code, 'request_used');
  // The used code is no good for a new request (here a restore).
  const restore = await restoreRequest(env);
  assert.equal(restore.status, 200, restore.text);
  const replayed = await restoreConfirm(env, restore.body.request_id, code);
  assert.equal(replayed.status, 401, replayed.text);
  assert.equal(replayed.body.code, 'code_invalid');
  assert.equal((await markOf(env)).state, 'soft', 'still deleted');
  // And once a new request has replaced it, the confirmed delete cannot even be replayed.
  const stale = await deleteConfirm(env, request.body.request_id, code);
  assert.equal(stale.status, 400, stale.text);
  assert.equal(stale.body.code, 'request_invalid');
});

test('a confirm from another session of the same account is refused', async () => {
  const env = await environment();
  const r = await deleteRequest(env);
  const other = mintToken(AUTHOR);
  await new Promise((done) => setTimeout(done, 1100));
  const second = mintToken(AUTHOR);
  assert.notEqual(other, second);
  const res = await worker.fetch(new Request(`${ORIGIN}/api/account/spaces/${SIM}/delete-confirm`, {
    method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${second}` }, body: JSON.stringify({ request_id: r.body.request_id, typed_name: NAME, code: lastCode(env) }),
  }), env, ctx);
  const body = await res.json();
  assert.equal(res.status, 400, JSON.stringify(body));
  assert.equal(body.code, 'request_invalid');
  assert.equal(await markOf(env), null);
});

// ── Rate limits ──────────────────────────────────────────────────────────────

test('three requests per listing an hour and five per account a day; the windows slide', async () => {
  const T = Date.now();
  const env = await environment({ sims: [SIM, SIM2, SIM3] });
  for (let i = 0; i < 3; i++) assert.equal((await deleteRequest(env, SIM, { at: T + i })).status, 200);
  const fourth = await deleteRequest(env, SIM, { at: T + 10 });
  assert.equal(fourth.status, 429, fourth.text);
  assert.equal(fourth.body.code, 'rate_limited');
  assert.ok(Number(fourth.headers.get('Retry-After')) > 0);
  assert.equal(env.EMAIL.sent.length, 3, 'the refused request mailed nothing');
  // An hour and a millisecond after the first, that listing may ask again: four on the account now.
  assert.equal((await deleteRequest(env, SIM, { at: T + HOUR + 1 })).status, 200);
  assert.equal((await deleteRequest(env, SIM2, { at: T + HOUR + 2 })).status, 200);
  const sixth = await deleteRequest(env, SIM3, { at: T + HOUR + 3 });
  assert.equal(sixth.status, 429, 'the sixth on the account in a day');
  assert.equal(sixth.body.code, 'rate_limited');
  assert.equal((await deleteRequest(env, SIM3, { at: T + DAY + 1 })).status, 200, 'a day later the account may ask again');
});

// ── Money ────────────────────────────────────────────────────────────────────

const PUR = (n) => `pur_${String(n).padStart(24, 'A')}`;
async function sellIn(env, { active = true, soldAgo = null, refunded = false, livemode = true } = {}) {
  const hub = env.COMMERCE_HUBS.get(`hub:${AUTHOR}`);
  const ask = async (op, args) => (await hub.fetch(`https://commerce.internal/${op}`, { method: 'POST', body: JSON.stringify({ account_id: AUTHOR, ...args }) })).json();
  const made = await ask('create_product', { sim_id: SIM, fields: { space: 'Main', name: 'Hat', type: 'pass', price: 100, active } });
  assert.equal(made.ok, true, JSON.stringify(made));
  if (soldAgo !== null) {
    const created = Math.floor((Date.now() - soldAgo) / 1000);
    const sale = await ask('record_sale', { purchase: { id: PUR(1), created, livemode, sim_id: SIM, sim_name: NAME, buyer_id: STRANGER, seller_id: AUTHOR, product: { id: made.product.id }, amount: 100, paid_amount: 0, creator_amount: 70, platform_amount: 30 } });
    assert.equal(sale.ok, true, JSON.stringify(sale));
    if (refunded) assert.equal((await ask('sale_refunded', { purchase_id: PUR(1) })).ok, true);
  }
  return { hub, ask, product: made.product };
}

test('a Space with a product on sale, or a live sale inside its 72-hour refund window, is refused in plain words', async () => {
  const onSale = await environment();
  await sellIn(onSale, { active: true });
  const r1 = await deleteRequest(onSale);
  assert.equal(r1.status, 409, r1.text);
  assert.deepEqual(r1.body, { code: 'products_on_sale', error: 'Take every product in this Space off sale first.', products_on_sale: 1 });
  assert.equal(onSale.EMAIL.sent.length, 0);

  const recent = await environment();
  await sellIn(recent, { active: false, soldAgo: 10 * HOUR });
  const r2 = await deleteRequest(recent);
  assert.equal(r2.status, 409, r2.text);
  assert.equal(r2.body.code, 'refund_window_open');
  assert.match(r2.body.error, /^You can delete this Space after .+, when the last refund window closes\.$/);
  assert.ok(Math.abs(Date.parse(r2.body.until) - (Date.now() - 10 * HOUR + 72 * HOUR)) < 2000);
  // The window as it closes: the same sale, asked about after the 72 hours.
  const after = await deleteRequest(recent, SIM, { at: Date.now() + 63 * HOUR });
  assert.equal(after.status, 200, after.text);

  const refunded = await environment();
  await sellIn(refunded, { active: false, soldAgo: HOUR, refunded: true });
  assert.equal((await deleteRequest(refunded)).status, 200, 'a refunded sale has no window left to protect');

  const test_mode = await environment();
  await sellIn(test_mode, { active: false, soldAgo: HOUR, livemode: false });
  assert.equal((await deleteRequest(test_mode)).status, 200, 'a test-mode sale moved no money');
});

test('a product put back on sale between the request and the confirm stops the confirm', async () => {
  const env = await environment();
  const { ask, product } = await sellIn(env, { active: false });
  const r = await deleteRequest(env);
  assert.equal(r.status, 200, r.text);
  assert.equal((await ask('update_product', { id: product.id, patch: { active: true } })).ok, true);
  const c = await deleteConfirm(env, r.body.request_id, lastCode(env));
  assert.equal(c.status, 409);
  assert.equal(c.body.code, 'products_on_sale');
  assert.equal(await markOf(env), null);
});

// ── Holds ────────────────────────────────────────────────────────────────────

/// Every kind of hold, each as the store holds it. `id` is the listing.
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

test('every kind of hold refuses the start with the one generic copy, and opens and mails nothing', async () => {
  for (const [kind, place] of Object.entries(HOLDS)) {
    const env = await environment();
    await place(env, SIM);
    const r = await deleteRequest(env);
    assert.equal(r.status, 409, `${kind}: ${r.text}`);
    const words = RETENTION_ONLY_KINDS.includes(kind) ? CANNOT_DELETE_NOW : IN_REVIEW;
    assert.deepEqual(r.body, words, `${kind}: the same words for every live hold, the neutral words for a retention marker alone`);
    assert.equal(env.EMAIL.sent.length, 0, `${kind}: nothing mailed`);
    assert.equal(pendingOf(env), null, `${kind}: nothing opened`);
    const status = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`);
    assert.deepEqual(status.body.blocked, words, `${kind}: the status read says the same`);
  }
});

test('a review, a takedown or a seal writing the case right now (the lease is held) refuses the start too', async () => {
  const env = await environment();
  await env.SCENES.put(`locks/moderation/${SIM}.json`, JSON.stringify({ holder: 'review-run', expires_at: Date.now() + 5 * MIN }));
  const r = await deleteRequest(env);
  assert.deepEqual(r.body, IN_REVIEW);
});

test('a hold placed between the request and the confirm stops the confirm; the code stays unspent', async () => {
  for (const kind of ['legal_lock', 'takedown', 'report_debt', 'staff_hold']) {
    const env = await environment();
    const r = await deleteRequest(env);
    await HOLDS[kind](env, SIM);
    const c = await deleteConfirm(env, r.body.request_id, lastCode(env));
    assert.deepEqual(c.body, RETENTION_ONLY_KINDS.includes(kind) ? CANNOT_DELETE_NOW : IN_REVIEW, kind);
    assert.equal(await markOf(env), null, `${kind}: not deleted`);
    assert.equal(pendingOf(env).attempts, 0, `${kind}: no attempt was spent`);
  }
});

// ── Serving: the mark is the truth ───────────────────────────────────────────

async function withManifest(env, id = SIM, ns = 'superstation') {
  await env.SCENES.put(`universes/${id}/website-manifest.json`, '{"schema_version":1,"values":{}}', { customMetadata: { simId: id, namespace: ns, authorId: AUTHOR, publishHash: 'h1' } });
  await env.SOCIAL.put(`website-ns:${ns}`, JSON.stringify({ sim_id: id, owner_id: AUTHOR, claimed_at: new Date().toISOString() }));
  await env.SOCIAL.put(`website-ns-of:${id}`, ns);
}

async function servedToStranger(env) {
  return {
    gallery: await inGallery(env),
    search: ((await call(env, 'GET', `/api/simulations?q=${encodeURIComponent('super')}`, { who: null })).body?.simulations || []).some((s) => s.id === SIM),
    page: (await call(env, 'GET', `/api/simulations/${SIM}`, { who: 'stranger' })).status,
    download: (await call(env, 'GET', `/api/simulations/${SIM}/download`, { who: 'stranger' })).status,
    thumbnail: (await call(env, 'GET', `/api/simulations/${SIM}/thumbnail`, { who: null })).status,
    manifest_by_id: (await call(env, 'GET', `/api/simulation/${SIM}/manifest`, { who: null })).status,
    manifest_by_namespace: (await call(env, 'GET', '/api/simulation/superstation/latest/manifest', { who: null })).status,
    catalog: (await call(env, 'GET', `/api/commerce/catalog/${SIM}`, { who: null })).status,
  };
}

test('a soft-deleted Space is hidden from everyone but its author, its bytes kept and its KV record untouched', async () => {
  const env = await environment();
  await withManifest(env);
  const before = await servedToStranger(env);
  assert.deepEqual(before, { gallery: true, search: true, page: 200, download: 200, thumbnail: 200, manifest_by_id: 200, manifest_by_namespace: 200, catalog: 200 }, 'served before');
  const record = await env.SOCIAL.get(`sim:${SIM}`);
  const objects = [...env.SCENES.objects.keys()].filter((k) => !k.startsWith('locks/')).sort();
  await softDelete(env);
  const after = await servedToStranger(env);
  assert.deepEqual(after, { gallery: false, search: false, page: 404, download: 403, thumbnail: 404, manifest_by_id: 404, manifest_by_namespace: 404, catalog: 404 });
  assert.equal(await env.SOCIAL.get(`sim:${SIM}`), record, 'the KV record is byte for byte what it was');
  assert.deepEqual([...env.SCENES.objects.keys()].filter((k) => !k.startsWith('locks/')).sort(), objects, 'every object is still there');
  // The author still reads their own world, to keep a copy.
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}`)).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`)).status, 200);
  const projects = await call(env, 'GET', '/api/projects');
  const card = projects.body.projects.find((p) => p.id === SIM);
  assert.equal(card.status, 'deleted');
  assert.equal(card.deletion.state, 'soft');
  assert.ok(card.deletion.restore_until);
  // The mark is an R2 object, made create-if-absent, with its state in its metadata.
  const mark = await markOf(env);
  assert.equal(mark.state, 'soft');
  assert.ok(MARK_STATES.includes(env.SCENES.objects.get(`locks/deleted/${SIM}.json`).meta.customMetadata.state));
  assert.equal(Date.parse(mark.hard_delete_after) - Date.parse(mark.soft_deleted_at), 90 * DAY);
});

test('the mark survives a whole-record KV rewrite: a writer that puts the old record back cannot bring the listing back', async () => {
  const env = await environment();
  await withManifest(env);
  const old = await env.SOCIAL.get(`sim:${SIM}`);
  await softDelete(env);
  // A moderation run (applyToSim) that read the record before the deletion and writes it now, with a fresh approval.
  const rewritten = JSON.parse(old);
  rewritten.moderation = { status: 'approved', rating: 'all_ages', updated_at: new Date().toISOString() };
  rewritten.is_public = true;
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(rewritten));
  assert.deepEqual(await servedToStranger(env), { gallery: false, search: false, page: 404, download: 403, thumbnail: 404, manifest_by_id: 404, manifest_by_namespace: 404, catalog: 404 });
  const upload = await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { body: testPng(), headers: { 'Content-Type': 'image/png' } });
  assert.equal(upload.status, 409);
  assert.equal(upload.body.code, 'listing_deleted');
});

test('every author write to a deleted listing answers listing_deleted; anyone else gets the 404; a host is told to let guests go', async () => {
  const env = await environment();
  await softDelete(env);
  const writes = [
    ['POST', `/api/simulations/${SIM}/archive`], ['POST', `/api/simulations/${SIM}/unarchive`], ['PATCH', `/api/simulations/${SIM}/tags`],
    ['PUT', `/api/simulations/${SIM}/space`], ['POST', `/api/simulations/${SIM}/space/multipart/create`], ['PUT', `/api/simulations/${SIM}/space/multipart/part`],
    ['POST', `/api/simulations/${SIM}/space/multipart/complete`], ['PUT', `/api/simulations/${SIM}/spaces/Main`], ['PUT', `/api/simulations/${SIM}/thumbnail`],
    ['PUT', `/api/simulations/${SIM}/website-manifest`], ['POST', `/api/simulations/${SIM}/world/begin`], ['PUT', `/api/simulations/${SIM}/cover`],
    ['DELETE', `/api/simulations/${SIM}/cover`], ['POST', `/api/simulations/${SIM}/changelogs`], ['PUT', `/api/simulations/${SIM}/dossier`],
    ['POST', `/api/simulations/${SIM}/submit`], ['POST', `/api/simulations/${SIM}/appeal`], ['POST', `/api/simulations/${SIM}/rating-questionnaire`],
    ['PUT', `/api/simulations/${SIM}/captures/0`], ['POST', `/api/simulations/${SIM}/live`], ['POST', `/api/simulations/${SIM}/live/admit`],
    ['POST', `/api/simulations/${SIM}/server-credential`], ['POST', `/api/simulations/${SIM}/play`],
  ];
  for (const [method, path] of writes) {
    const r = await call(env, method, path, { body: {} });
    assert.equal(r.status, 409, `${method} ${path}: ${r.text}`);
    assert.equal(r.body.code, 'listing_deleted', `${method} ${path}`);
    assert.equal(r.body.error, 'This Space is deleted. Restore it from Projects to change it.');
    assert.ok(r.body.restore_until);
    const s = await call(env, method, path, { who: 'stranger', body: {} });
    assert.equal(s.status, 404, `${method} ${path} by a stranger`);
  }
  const heartbeat = await call(env, 'POST', `/api/simulations/${SIM}/live`, { who: null, headers: { Authorization: `Bearer esrv_${'a'.repeat(64)}` }, body: {} });
  assert.equal(heartbeat.status, 404);
  assert.equal(heartbeat.body.end_all_guests, true);
  const own = await call(env, 'POST', `/api/simulations/${SIM}/live`, { body: {} });
  assert.equal(own.body.end_all_guests, true, 'the author\'s own host lets every guest go too');
  // A product cannot be made or sold in it.
  const product = await call(env, 'POST', '/api/commerce/products', { body: { sim_id: SIM, space: 'Main', name: 'Hat', type: 'pass', price: 100 } });
  assert.equal(product.status, 409, product.text);
  // A hold answers first, in its own words.
  await HOLDS.legal_lock(env, SIM);
  const held = await call(env, 'POST', `/api/simulations/${SIM}/archive`, { body: {} });
  assert.deepEqual(held.body, IN_REVIEW);
});

// ── Restore ──────────────────────────────────────────────────────────────────

test('the owner restores within ninety days, with the emailed code, and gets exactly the listing that was there', async () => {
  const T = Date.now();
  const env = await environment();
  const record = await env.SOCIAL.get(`sim:${SIM}`);
  await softDelete(env, SIM, { at: T });
  const r = await restoreRequest(env, SIM, { at: T + 89 * DAY });
  assert.equal(r.status, 200, r.text);
  assert.equal(r.body.action, 'restore');
  const c = await restoreConfirm(env, r.body.request_id, lastCode(env), SIM, { at: T + 89 * DAY + MIN });
  assert.equal(c.status, 200, c.text);
  assert.equal(c.body.state, 'restored');
  assert.equal(await markOf(env), null, 'the mark is gone');
  assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('spacedel:due:')).length, 0, 'and its due key');
  assert.equal(await env.SOCIAL.get(`sim:${SIM}`), record, 'the record is exactly what it was');
  assert.equal(await inGallery(env), true, 'it serves again');
  const notice = env.EMAIL.sent.filter((m) => subjectOf(m) === 'A Space on your Eustress account was restored');
  assert.equal(notice.length, 1);
  // The job finds nothing to do.
  const job = await SD.sweepSpaceDeletions(env, {}, { now: T + 91 * DAY });
  assert.equal(job.hard_deleted, 0);
  assert.ok(env.SCENES.objects.has(`universes/${SIM}/universe.pak`));
});

test('restore is refused once the ninety days are over, once removal has begun, under a hold, and for a stranger', async () => {
  const T = Date.now();
  const env = await environment();
  await softDelete(env, SIM, { at: T });
  const closed = await restoreRequest(env, SIM, { at: T + 90 * DAY });
  assert.equal(closed.status, 410, closed.text);
  assert.equal(closed.body.code, 'restore_window_closed');
  // A code asked for in time and typed a moment too late is refused too.
  const asked = await restoreRequest(env, SIM, { at: T + 90 * DAY - 2 * MIN });
  const late = await restoreConfirm(env, asked.body.request_id, lastCode(env), SIM, { at: T + 90 * DAY });
  assert.equal(late.status, 410);
  assert.equal(late.body.code, 'restore_window_closed');
  assert.equal((await markOf(env)).state, 'soft');
  await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + HOUR, budget: 6 });
  assert.equal((await markOf(env)).state, 'hard_deleting', 'removal has begun');
  const started = await restoreRequest(env, SIM, { at: T + 90 * DAY + 2 * HOUR });
  assert.equal(started.status, 410);
  assert.equal(started.body.code, 'removal_started');

  const held = await environment();
  await softDelete(held, SIM, { at: T });
  await HOLDS.takedown(held, SIM);
  assert.deepEqual((await restoreRequest(held, SIM, { at: T + DAY })).body, IN_REVIEW);
  assert.equal((await restoreRequest(held, SIM, { at: T + DAY, who: 'stranger' })).status, 404);
});

// ── The hourly job ───────────────────────────────────────────────────────────

/// Everything a listing owns in money, evidence, cases and audit, which no deletion may touch.
async function kept(env) {
  await env.INVENTORY.put(`txn:${STRANGER}:1700000000000:${PUR(1)}`, JSON.stringify({ simulation_id: SIM, amount: -100 }));
  await env.INVENTORY.put(`purchase:${STRANGER}:8299999999999:${PUR(1)}`, JSON.stringify({ simulation_id: SIM, name: NAME }));
  await env.PAYOUTS.put(`payout:${AUTHOR}:2026-10-01`, '{"usd":1}');
  const wallet = env.WALLETS.get(`wallet:${STRANGER}`);
  await wallet.fetch('https://commerce.internal/balance', { method: 'POST', body: JSON.stringify({ account_id: STRANGER }) });
  const ws = env.WALLETS.instances.get(`wallet:${STRANGER}`).storage;
  await ws.put(`pur:${PUR(1)}`, { id: PUR(1), sim_id: SIM });
  await ws.put(`ent:live:${SIM}:prod_x`, { sim_id: SIM });
  await env.SOCIAL.put(`caseref-of:moderation:${SIM}`, REF);
  await env.SOCIAL.put(`caseref:${REF}`, JSON.stringify({ kind: 'moderation', id: SIM }));
  await env.SCENES.put(`caserefs/moderation/${SIM}`, REF);
  await env.SCENES.put(`evidence/${REF}/pak.json`, JSON.stringify({ copied: true, never_delete: false }));
  await env.SCENES.put(`evidence/${REF}/pak.c0`, new Uint8Array([9, 9, 9]));
  await env.SCENES.put(`evidence-views/${REF}/${ADMIN}.json`, '{}');
  await env.SCENES.put(`derived/stripped/${'d'.repeat(64)}`, new Uint8Array([7]));
  await env.SOCIAL.put(`legalcase:ncmec:moderation:${SIM}`, '{"sent":true}');
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, status: 'approved', created_at: '2026-10-01T00:00:00.000Z', updated_at: '2026-10-02T00:00:00.000Z' }));
  await env.SOCIAL.put(`modq:approved:${SIM}`, '2026-10-02T00:00:00.000Z');
  await env.SCENES.put(`locks/approved/${SIM}.json`, '{"content":"x"}');
  await env.SCENES.put(`locks/state/${SIM}.json`, '{"epoch":0}');
  await env.AUDIT_LOG.put('audit:1-old', '{"action":"OLD"}');
}

const KEPT_PREFIXES = ['evidence/', 'evidence-views/', 'derived/', 'caserefs/', 'locks/legal/', 'locks/state/', `universes/${SIM}/moderation/`];
function snapshot(env) {
  const r2 = Object.fromEntries([...env.SCENES.objects].filter(([k]) => KEPT_PREFIXES.some((p) => k.startsWith(p))).map(([k, o]) => [k, Buffer.from(o.body).toString('hex')]));
  const kv = (ns, keep) => Object.fromEntries([...ns.store].filter(([k]) => keep(k)));
  return {
    r2,
    inventory: kv(env.INVENTORY, () => true),
    payouts: kv(env.PAYOUTS, () => true),
    social: kv(env.SOCIAL, (k) => /^(caseref|legalcase|legalowed|tdcase|tdlisting|ncii)/.test(k)),
    audit_old: env.AUDIT_LOG.store.get('audit:1-old'),
    wallet: JSON.stringify([...env.WALLETS.instances.get(`wallet:${STRANGER}`).storage.map].sort()),
  };
}

test('the job removes a due listing in pages, keeps money, evidence, cases and audit byte for byte, and leaves a tombstone with the file hashes', async () => {
  const T = Date.now();
  const env = await environment();
  await withManifest(env);
  await kept(env);
  await env.SOCIAL.put(`plays:${SIM}`, '5');
  await env.SOCIAL.put(`visits:${SIM}`, '9');
  await env.SOCIAL.put(`srvcred-sim:${SIM}`, 'e'.repeat(64));
  await env.SOCIAL.put(`srvcred:${'e'.repeat(64)}`, JSON.stringify({ sim_id: SIM }));
  for (let i = 0; i < 2400; i++) await env.SCENES.put(`universes/${SIM}/chunks/${String(i).padStart(64, '0')}.echk`, new Uint8Array([i & 255]));
  const { ask } = await sellIn(env, { active: false, soldAgo: 5 * DAY });
  await softDelete(env, SIM, { at: T });
  const before = snapshot(env);
  const saleBefore = await ask('get_sale', { purchase_id: PUR(1) });

  const early = await SD.sweepSpaceDeletions(env, {}, { now: T + 89 * DAY });
  assert.equal(early.hard_deleted, 0, 'a listing not yet due is untouched');
  assert.ok(env.SCENES.objects.has(`universes/${SIM}/universe.pak`));

  // ADJUSTED (fix L6): every removed file's SHA-256 is taken before it goes, within the run's call allowance, so 2,404 files
  // now take several hourly runs instead of one. The bytes are hashed here first, to check what the tombstone records.
  const expected = new Map([...env.SCENES.objects].filter(([k]) => SD.removedPrefixes(SIM).some((p) => k.startsWith(p)) && !k.startsWith(`universes/${SIM}/moderation/`)).map(([k, o]) => [k, crypto.createHash('sha256').update(o.body).digest('hex')]));
  let job;
  let runs = 0;
  for (let hour = 1; hour <= 20 && !job?.hard_deleted; hour++) { job = await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + hour * HOUR }); runs++; }
  assert.equal(job.hard_deleted, 1, JSON.stringify(job));
  assert.ok(runs > 1, `bounded: ${runs} runs`);
  for (const prefix of [`universes/${SIM}/chunks/`, `universes/${SIM}/spaces/`, `thumbnails/${SIM}/`, `covers/${SIM}/`]) {
    assert.equal([...env.SCENES.objects.keys()].filter((k) => k.startsWith(prefix)).length, 0, prefix);
  }
  assert.ok(!env.SCENES.objects.has(`universes/${SIM}/universe.pak`));
  assert.ok(!env.SCENES.objects.has(`universes/${SIM}/website-manifest.json`));
  assert.ok(!env.SCENES.objects.has(`locks/approved/${SIM}.json`));
  for (const key of [`sim:${SIM}`, `sim-author:${AUTHOR}:${SIM}`, `plays:${SIM}`, `visits:${SIM}`, `website-ns-of:${SIM}`, `srvcred-sim:${SIM}`, `srvcred:${'e'.repeat(64)}`, `modq:approved:${SIM}`]) {
    assert.equal(await env.SOCIAL.get(key), null, key);
  }
  assert.equal(await env.SOCIAL.get(`simCount:${AUTHOR}`), '0');
  assert.deepEqual(snapshot(env), before, 'money, evidence, cases, review files and old audit are byte for byte as they were');
  const saleAfter = await ask('get_sale', { purchase_id: PUR(1) });
  assert.deepEqual(saleAfter, saleBefore, 'the sale record is kept');
  const products = await ask('list_products', { sim_id: SIM, all: true });
  assert.equal(products.data.length, 1, 'the product record is kept');
  assert.equal(products.data[0].active, false);
  // The case is kept and stamped; its reference still resolves.
  const rec = JSON.parse(await env.SOCIAL.get(`modcase:${SIM}`));
  assert.equal(rec.status, 'approved');
  assert.ok(rec.listing_deleted?.at);
  const byRef = await call(env, 'GET', `/api/cases/${REF}`, { who: null });
  assert.equal(byRef.status, 200, byRef.text);
  // The tombstone, and the hash of every removed file, page by page.
  const tomb = JSON.parse(await env.SOCIAL.get(`simgone:${SIM}`));
  assert.equal(tomb.name, NAME);
  assert.equal(tomb.case_reference, REF);
  assert.equal(await env.SOCIAL.get(`simgone-author:${AUTHOR}:${SIM}`), SIM);
  const pages = [...env.SCENES.objects.keys()].filter((k) => k.startsWith(`tombstones/${SIM}/`));
  assert.ok(pages.length >= 3, `${pages.length} hash pages`);
  const listed = (await Promise.all(pages.map(async (k) => JSON.parse(await (await env.SCENES.get(k)).text()).files))).flat();
  assert.equal(listed.length, 2400 + 4, 'every removed object is listed: the chunks, the .pak, the Space, the manifest, the thumbnail');
  // ADJUSTED (fix L6): and each with the SHA-256 of its bytes, once.
  assert.equal(new Set(listed.map((f) => f.key)).size, listed.length, 'each file once');
  for (const f of listed) assert.equal(f.sha256, expected.get(f.key), `${f.key}: ${f.sha256_from}`);
  assert.ok(tomb.file_hashes.some((h) => h.kind === 'package' && h.sha256 === expected.get(`universes/${SIM}/universe.pak`)), JSON.stringify(tomb.file_hashes));
  assert.ok(!tomb.file_hashes.some((h) => h.kind === 'chunk'), 'chunks are not copied to the tombstone record');
  // The mark stays forever, and the author's case list still shows the review.
  const mark = await markOf(env);
  assert.equal(mark.state, 'hard_deleted');
  const cases = await call(env, 'GET', '/api/account/cases');
  const entry = cases.body.cases.find((c) => c.listing?.id === SIM);
  assert.ok(entry, cases.text);
  assert.equal(entry.listing.name, NAME);
  assert.ok(entry.listing.deleted_at);
  // A later write to the id is still refused, and the status read says it is gone.
  assert.equal((await call(env, 'PUT', `/api/simulations/${SIM}/thumbnail`, { body: testPng(), headers: { 'Content-Type': 'image/png' } })).body.code, 'listing_deleted');
  const status = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`);
  assert.equal(status.body.state, 'hard_deleted');
});

test('the website namespace stays reserved to its owner after the removal', async () => {
  const T = Date.now();
  const env = await environment({ sims: [SIM, SIM2] });
  await withManifest(env);
  await softDelete(env, SIM, { at: T });
  await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + HOUR });
  const claim = JSON.parse(await env.SOCIAL.get('website-ns:superstation'));
  assert.equal(claim.owner_id, AUTHOR);
  assert.equal(claim.reserved, true);
  assert.equal((await call(env, 'GET', '/api/simulation/superstation/latest/manifest', { who: null })).status, 404);
  // Another author cannot take it.
  await env.SOCIAL.put(`sim:${SIM3}`, JSON.stringify({ ...listing(SIM3), author_id: STRANGER }));
  const taken = await call(env, 'PUT', `/api/simulations/${SIM3}/website-manifest`, { who: 'stranger', body: { schema_version: 1, namespace: 'superstation', publish_hash: 'h2', values: {} } });
  assert.equal(taken.status, 409, taken.text);
  assert.equal(taken.body.error, 'Namespace already claimed');
  assert.equal(JSON.parse(await env.SOCIAL.get('website-ns:superstation')).owner_id, AUTHOR, 'the claim is still the owner\'s');
  // Its owner can use it again for another listing.
  const reused = await call(env, 'PUT', `/api/simulations/${SIM2}/website-manifest`, { body: { schema_version: 1, namespace: 'superstation', publish_hash: 'h3', values: {} } });
  assert.equal(reused.status, 200, reused.text);
  assert.equal((await call(env, 'GET', '/api/simulation/superstation/latest/manifest', { who: null })).status, 200);
});

test('every hold defers the removal, with its reason on the mark, and nothing is removed', async () => {
  const T = Date.now();
  for (const [kind, place] of Object.entries(HOLDS)) {
    const env = await environment();
    await softDelete(env, SIM, { at: T });
    await place(env, SIM);
    const objects = [...env.SCENES.objects.keys()].filter((k) => !k.startsWith('locks/')).sort();
    const job = await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + HOUR });
    assert.equal(job.hard_deleted, 0, `${kind}: ${JSON.stringify(job)}`);
    assert.equal(Object.values(job.deferred).reduce((a, b) => a + b, 0), 1, `${kind}: deferred`);
    const mark = await markOf(env);
    assert.equal(mark.state, 'soft', kind);
    assert.ok(mark.deferred?.reason, kind);
    assert.deepEqual([...env.SCENES.objects.keys()].filter((k) => !k.startsWith('locks/')).sort(), objects, `${kind}: every object is still there`);
    assert.ok(await env.SOCIAL.get(`sim:${SIM}`), `${kind}: the record is still there`);
    // The author is told nothing about which: the Projects card says only that removal is in progress.
    const status = await direct(env, 'GET', `/api/account/spaces/${SIM}/deletion`, { at: T + 90 * DAY + HOUR });
    assert.equal(status.body.deletion.removal_in_progress, true, kind);
    assert.ok(!('deferred' in status.body.deletion), `${kind}: no reason reaches the owner`);
    assert.ok(!JSON.stringify(status.body).includes(mark.deferred.reason), `${kind}: the reason ${mark.deferred.reason} is not in the owner's answer`);
  }
});

test('the legal lock is read again before every R2 delete call: a lock that appears between pages stops the job there', async () => {
  const T = Date.now();
  const env = await environment();
  for (let i = 0; i < 2500; i++) await env.SCENES.put(`universes/${SIM}/chunks/${String(i).padStart(64, '0')}.echk`, new Uint8Array([1]));
  await softDelete(env, SIM, { at: T });
  let deletes = 0;
  env.SCENES.beforeDelete = async (keys) => {
    if (!Array.isArray(keys)) return;
    deletes++;
    // A TAKE IT DOWN request lands while the first page is being deleted.
    if (deletes === 1) await HOLDS.legal_lock(env, SIM);
  };
  // ADJUSTED (fix L6): each file is read for its SHA-256 within the run's call allowance; this run gets room for whole pages
  // of 1,000, so the lock still lands between pages as before.
  const job = await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + HOUR, budget: 100_000 });
  assert.equal(job.deferred.legal_lock, 1, JSON.stringify(job));
  assert.equal(deletes, 1, 'no delete call after the lock appeared');
  const left = [...env.SCENES.objects.keys()].filter((k) => k.startsWith(`universes/${SIM}/chunks/`)).length;
  assert.equal(left, 1500, 'the first page went; the rest is left in place');
  const mark = await markOf(env);
  assert.equal(mark.state, 'hard_deleting');
  assert.equal(mark.deferred.reason, 'legal_lock');
  assert.ok(await env.SOCIAL.get(`sim:${SIM}`), 'the record is still there, so the takedown finds it');
  // Lifted by a person: the job finishes from where it stopped.
  await env.SCENES.delete(`locks/legal/${SIM}`);
  env.SCENES.beforeDelete = null;
  const resumed = await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + 2 * HOUR, budget: 100_000 }); // ADJUSTED (fix L6): room for the rest
  assert.equal(resumed.hard_deleted, 1, JSON.stringify(resumed));
  assert.equal([...env.SCENES.objects.keys()].filter((k) => k.startsWith(`universes/${SIM}/chunks/`)).length, 0);
});

test('the job is idempotent, resumes after a failure from its saved place, and keeps within its call budget', async () => {
  const T = Date.now();
  const env = await environment();
  for (let i = 0; i < 3500; i++) await env.SCENES.put(`universes/${SIM}/chunks/${String(i).padStart(64, '0')}.echk`, new Uint8Array([1]));
  await softDelete(env, SIM, { at: T });
  // R2 fails after two delete calls.
  env.SCENES.failDeleteAfter = 2;
  env.SCENES.calls.pageDeletes = 0;
  // ADJUSTED (fix L6): room for whole pages of 1,000 hashed files, so the failure still comes at the third page.
  const first = await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + HOUR, budget: 100_000 });
  assert.equal(first.failed, 1, JSON.stringify(first));
  const mark = await markOf(env);
  assert.equal(mark.state, 'hard_deleting');
  assert.equal(mark.progress.phase, 'r2');
  assert.equal(mark.progress.objects, 2000, 'two pages were done and saved');
  env.SCENES.failDeleteAfter = Infinity;
  // A small budget: each run does a bounded amount and saves where it is.
  const listBefore = env.SCENES.calls.list;
  const second = await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + 2 * HOUR, budget: 20 });
  assert.ok(second.progressed === 1 || second.hard_deleted === 1, JSON.stringify(second));
  assert.ok(env.SCENES.calls.list - listBefore <= 20);
  // ADJUSTED (fix L6): a run of 20 calls now hashes a file or two, so the rest is finished with a larger allowance.
  for (let run = 0; run < 30 && (await markOf(env)).state !== 'hard_deleted'; run++) await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + (3 + run) * HOUR, budget: 200 });
  assert.equal((await markOf(env)).state, 'hard_deleted');
  assert.equal([...env.SCENES.objects.keys()].filter((k) => k.startsWith(`universes/${SIM}/chunks/`)).length, 0);
  const hashes = (await Promise.all([...env.SCENES.objects.keys()].filter((k) => k.startsWith(`tombstones/${SIM}/`)).map(async (k) => JSON.parse(await (await env.SCENES.get(k)).text()).files))).flat();
  assert.equal(new Set(hashes.map((f) => f.key)).size, 3500 + 3, 'every removed object listed once');
  // Run again: nothing changes.
  const objects = JSON.stringify([...env.SCENES.objects.keys()].sort());
  const social = JSON.stringify([...env.SOCIAL.store].sort());
  const again = await SD.sweepSpaceDeletions(env, {}, { now: T + 95 * DAY });
  assert.equal(again.hard_deleted, 0);
  assert.equal(JSON.stringify([...env.SCENES.objects.keys()].sort()), objects);
  assert.equal(JSON.stringify([...env.SOCIAL.store].sort()), social);
});

test('the job never re-creates a mark that a restore removed while it waited for the lease', async () => {
  const T = Date.now();
  const env = await environment();
  await softDelete(env, SIM, { at: T });
  // The restore finishes just as the job takes the lease.
  const realHead = env.SCENES.get;
  let first = true;
  env.SCENES.get = async (k) => {
    const o = await realHead(k);
    if (first && k === `locks/deleted/${SIM}.json`) { first = false; env.SCENES.objects.delete(k); }
    return o;
  };
  const job = await SD.sweepSpaceDeletions(env, {}, { now: T + 90 * DAY + HOUR });
  env.SCENES.get = realHead;
  assert.equal(job.dropped, 1, JSON.stringify(job));
  assert.equal(await markOf(env), null, 'no mark came back');
  assert.ok(env.SCENES.objects.has(`universes/${SIM}/universe.pak`));
});

// ── Staff ────────────────────────────────────────────────────────────────────

test('a staff hold needs an administrator and a rationale, is audited, refuses a start and defers a removal', async () => {
  const env = await environment();
  assert.equal((await call(env, 'POST', `/api/admin/space-deletions/${SIM}/hold`, { who: 'stranger', body: { rationale: 'x'.repeat(30) } })).status, 403);
  const short = await call(env, 'POST', `/api/admin/space-deletions/${SIM}/hold`, { who: 'admin', body: { rationale: 'too short' } });
  assert.equal(short.status, 400);
  const held = await call(env, 'POST', `/api/admin/space-deletions/${SIM}/hold`, { who: 'admin', body: { rationale: 'Counsel asked to keep this listing intact.' } });
  assert.equal(held.status, 200, held.text);
  assert.ok([...env.AUDIT_LOG.store.values()].some((v) => JSON.parse(v).action === 'SPACE_DELETE_STAFF_HOLD'));
  assert.deepEqual((await deleteRequest(env)).body, IN_REVIEW);
  const released = await call(env, 'POST', `/api/admin/space-deletions/${SIM}/release-hold`, { who: 'admin', body: { rationale: 'Counsel released the request today.' } });
  assert.equal(released.status, 200);
  assert.equal((await deleteRequest(env)).status, 200);
  const list = await call(env, 'GET', '/api/admin/space-deletions', { who: 'admin' });
  assert.equal(list.status, 200);
  assert.equal((await call(env, 'GET', '/api/admin/space-deletions', { who: 'author' })).status, 403);
});

// ── The code is never written anywhere but the one mail ──────────────────────

test('the code is never logged, audited, stored or answered: it is in the one mail and nowhere else', async () => {
  const env = await environment();
  const lines = [];
  const saved = {};
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
    saved[level] = console[level];
    console[level] = (...args) => { lines.push(args.map((a) => (typeof a === 'string' ? a : (() => { try { return JSON.stringify(a); } catch { return String(a); } })())).join(' ')); };
  }
  const answers = [];
  let codes = [];
  try {
    const r = await deleteRequest(env);
    answers.push(r.text);
    codes.push(lastCode(env));
    const wrong = await deleteConfirm(env, r.body.request_id, String((Number(codes[0]) + 3) % 1_000_000).padStart(6, '0'));
    answers.push(wrong.text);
    const c = await deleteConfirm(env, r.body.request_id, codes[0]);
    answers.push(c.text);
    const rr = await restoreRequest(env);
    answers.push(rr.text);
    codes.push(lastCode(env));
    const rc = await restoreConfirm(env, rr.body.request_id, codes[1]);
    answers.push(rc.text);
    assert.equal(rc.status, 200, rc.text);
  } finally {
    for (const level of Object.keys(saved)) console[level] = saved[level];
  }
  assert.equal(codes.filter(Boolean).length, 2);
  const word = (code) => new RegExp(`(?<![0-9A-Za-z])${code}(?![0-9A-Za-z])`);
  const stored = [
    ...[...env.ACCOUNT_AUTH.instances.values()].flatMap((o) => [...o.storage.map.values()].map((v) => JSON.stringify(v))),
    ...[...env.AUDIT_LOG.store.values()], ...[...env.SOCIAL.store.values()], ...[...env.USERS.store.values()],
    ...[...env.SCENES.objects.values()].map((o) => Buffer.from(o.body).toString()),
  ];
  for (const code of codes) {
    assert.ok(!lines.some((l) => word(code).test(l)), 'not in any console line');
    assert.ok(!answers.some((a) => word(code).test(a)), 'not in any answer');
    assert.ok(!stored.some((s) => word(code).test(s)), 'not in any store: object state, audit, KV, R2');
  }
  // The mails: the code in the body of its own mail, never in a subject.
  const mails = env.EMAIL.sent.filter((m) => subjectOf(m) === 'Your Eustress confirmation code');
  assert.equal(mails.length, 2);
  assert.ok(mails.every((m, i) => bodyOf(m).includes(codes[i]) && !subjectOf(m).includes(codes[i])));
  assert.ok(env.EMAIL.sent.every((m) => !/Super Station/.test(subjectOf(m))), 'no subject names the Space');
  // What the object keeps of a code is a 64-hex digest.
  const state = env.ACCOUNT_AUTH.instances.get(AUTHOR).storage.map.get('state');
  assert.match(state.space_delete.pending[SIM].code_digest, /^[0-9a-f]{64}$/);
});

test('the soft delete tells every address the account has, and the notice names no code and carries no link that acts', async () => {
  const env = await environment();
  await softDelete(env);
  const notice = env.EMAIL.sent.filter((m) => subjectOf(m) === 'A Space on your Eustress account was deleted');
  assert.equal(notice.length, 1);
  const text = bodyOf(notice[0]);
  assert.match(text, /The Space "The Super Station" on your Eustress account was deleted on/);
  assert.match(text, /removed for good on/);
  assert.ok(!/https?:\/\/[^\s"]*(restore|delete)/i.test(text), 'no link that restores or deletes');
  // The audit chain in the object records the request, the confirm and the soft delete; AUDIT_LOG the soft delete.
  const types = [...env.ACCOUNT_AUTH.instances.get(AUTHOR).storage.map.entries()].filter(([k]) => k.startsWith('audit:')).map(([, v]) => v.type);
  for (const t of ['space_delete_requested', 'space_delete_confirmed', 'space_soft_deleted']) assert.ok(types.includes(t), t);
  assert.ok([...env.AUDIT_LOG.store.values()].some((v) => JSON.parse(v).action === 'SPACE_SOFT_DELETED'));
});

test('the status read gives the modals what they need', async () => {
  const env = await environment();
  await withManifest(env);
  const s = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`);
  assert.equal(s.status, 200, s.text);
  assert.equal(s.body.can_delete, true);
  assert.equal(s.body.blocked, null);
  assert.equal(s.body.soft_delete_days, 90);
  assert.equal(s.body.email_code.sent_to, 'o***@example.com');
  assert.equal(s.body.removes.website_namespace, 'superstation');
  assert.equal(s.headers.get('Cache-Control'), 'private, no-store');
  const r = await deleteRequest(env);
  const p = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`);
  assert.equal(p.body.pending.request_id, r.body.request_id);
  assert.equal(p.body.pending.attempts_left, 5);
  assert.ok(!JSON.stringify(p.body).includes('digest'));
  await deleteConfirm(env, r.body.request_id, lastCode(env));
  const d = await call(env, 'GET', `/api/account/spaces/${SIM}/deletion`);
  assert.equal(d.body.state, 'soft_deleted');
  assert.equal(d.body.can_restore, true);
  assert.equal(d.body.can_delete, false);
});

test('deleting is refused during the pause after a key replacement', async () => {
  const env = await environment();
  const state = env.ACCOUNT_AUTH.instances.get(AUTHOR).storage.map.get('state');
  state.value_frozen_until = Date.now() + 3 * DAY;
  env.ACCOUNT_AUTH.instances.get(AUTHOR).storage.map.set('state', state);
  const r = await deleteRequest(env);
  assert.equal(r.status, 423, r.text);
  assert.equal(r.body.code, 'account_frozen');
  assert.equal(env.EMAIL.sent.length, 0);
});

test('a code that could not be sent ends the request and says so', async () => {
  const env = await environment();
  env.EMAIL.send = async () => { throw new Error('E_DELIVERY_FAILED for owner@example.com'); };
  const saved = console.error;
  console.error = () => {};
  let r;
  try { r = await deleteRequest(env); } finally { console.error = saved; }
  assert.equal(r.status, 503, r.text);
  assert.equal(r.body.code, 'code_not_sent');
  assert.equal(pendingOf(env).status, 'cancelled');
});

// ── The author's case list after removals stays inside the invocation's storage budget ──────────────────────

test('the case list reads the tombstones page by page, bounded, and the whole request stays under 1,000 storage operations', async () => {
  const env = fakeEnv();
  // KV's list as Cloudflare documents it, at most 40 keys a page here, with a cursor until list_complete.
  env.SOCIAL.list = async ({ prefix = '', limit = 1000, cursor } = {}) => {
    const all = [...env.SOCIAL.store.keys()].filter((k) => k.startsWith(prefix)).sort();
    const start = cursor ? Number(cursor) : 0;
    const keys = all.slice(start, start + Math.min(limit, 1000, 40)).map((name) => ({ name }));
    const next = start + keys.length;
    return next < all.length ? { keys, list_complete: false, cursor: String(next) } : { keys, list_complete: true };
  };
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  const idOf = (n) => `aaaaaaaa-0000-4000-8000-${String(n).padStart(12, '0')}`;
  const caseOf = (id, n) => JSON.stringify({ sim_id: id, author_id: AUTHOR, status: 'approved', history: [], created_at: new Date(Date.UTC(2026, 9, 1) + n * 60_000).toISOString(), updated_at: new Date(Date.UTC(2026, 9, 1) + n * 60_000).toISOString() });
  for (let n = 0; n < 400; n++) {
    const id = idOf(n);
    await env.SOCIAL.put(`sim:${id}`, JSON.stringify({ id, name: `World ${n}`, author_id: AUTHOR, is_public: true, moderation: { status: 'approved' } }));
    await env.SOCIAL.put(`sim-author:${AUTHOR}:${id}`, id);
    await env.SOCIAL.put(`modcase:${id}`, caseOf(id, n));
  }
  for (let n = 1000; n < 1100; n++) {
    const id = idOf(n);
    await env.SOCIAL.put(`simgone:${id}`, JSON.stringify({ id, author_id: AUTHOR, name: `Gone ${n}`, deleted_at: '2027-01-01T00:00:00.000Z' }));
    await env.SOCIAL.put(`simgone-author:${AUTHOR}:${id}`, id);
    await env.SOCIAL.put(`modcase:${id}`, caseOf(id, n));
  }
  let ops = 0;
  for (const name of ['USERS', 'SOCIAL', 'INVENTORY', 'AUDIT_LOG', 'SCREENING', 'TELEMETRY', 'SCENES']) {
    for (const method of ['get', 'put', 'delete', 'list', 'head']) {
      if (typeof env[name]?.[method] !== 'function') continue;
      const inner = env[name][method].bind(env[name]);
      env[name][method] = (...args) => { ops += 1; return inner(...args); };
    }
  }
  const r = await call(env, 'GET', '/api/account/cases?limit=50');
  assert.equal(r.status, 200, r.text);
  assert.ok(ops < 1000, `the case list made ${ops} storage operations`);
  // The newest rows are the removed listings' reviews (their cases changed last); a bounded number of them is read.
  const gone = r.body.cases.filter((c) => c.listing?.deleted_at);
  assert.ok(gone.length > 0 && gone.length <= 40, `${gone.length} removed listings shown`);
  assert.ok(r.body.total > 200, `the live listings and the tombstones are both read (${r.body.total})`);
  // Fifty tombstones and no live listing: forty are read, across two KV pages.
  for (let n = 1000; n < 1100; n++) if (n >= 1050) { await env.SOCIAL.delete(`simgone-author:${AUTHOR}:${idOf(n)}`); }
  for (let n = 0; n < 400; n++) await env.SOCIAL.delete(`sim-author:${AUTHOR}:${idOf(n)}`);
  const small = await call(env, 'GET', '/api/account/cases?limit=50');
  assert.equal(small.body.total, 40, 'the bound of 40 holds');
});

test('a mark another deletion wrote after the check is never overwritten: the mark is made create-if-absent', async () => {
  const env = await environment();
  const other = { v: 1, sim_id: SIM, author_id: AUTHOR, name: NAME, state: 'soft', request_id: 'sdr_OTHEROTHEROTHEROTHEROTH', soft_deleted_at: new Date().toISOString(), hard_delete_after: new Date(Date.now() + 90 * DAY).toISOString() };
  const r = await deleteRequest(env);
  assert.equal(r.status, 200, r.text);
  // The other deletion lands after this confirm has looked for a mark and found none.
  await env.SCENES.put(`locks/deleted/${SIM}.json`, JSON.stringify(other), { customMetadata: { state: 'soft', author: AUTHOR, soft_deleted_at: other.soft_deleted_at, hard_delete_after: other.hard_delete_after, request_id: other.request_id } });
  const realHead = env.SCENES.head;
  env.SCENES.head = async (k) => (k === `locks/deleted/${SIM}.json` ? null : realHead(k));
  const c = await deleteConfirm(env, r.body.request_id, lastCode(env));
  env.SCENES.head = realHead;
  assert.equal(c.status, 409, c.text);
  assert.equal(c.body.code, 'already_deleted');
  assert.deepEqual(await markOf(env), other, 'the mark that was there first is untouched');
});
