// Who may call what on the account, KYC and moderation-data routes.
//
// Each test names a way the route used to hand something to the wrong caller:
// delete any account by its public username, read any KYC_STATUS key with no
// token, read screening results with no token, or write a KYC record under a
// registered account's id. The controls (a fresh registration session still
// uploads, submits and polls) keep the fixes honest about the real flow.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, fakeEnv, fakeKv, fakeBucket, mintToken, ctx } from './helpers/load_worker.mjs';
import { startKycSession, uploadTokenOf } from './helpers/kyc_session.mjs';

const worker = await loadWorker();

const JPEG = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1]);
const API = 'https://api.eustress.dev';

function environment(extra = {}) {
  return { ...fakeEnv(), KYC_STATUS: fakeKv(), KYC_BUCKET: fakeBucket(), ...extra };
}

async function call(env, method, path, { token, body, headers = {} } = {}) {
  const init = { method, headers: { ...headers } };
  if (token) init.headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) {
    init.headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const res = await worker.fetch(new Request(`${API}${path}`, init), env, ctx);
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* not json */ }
  return { status: res.status, json, text };
}

async function seedUser(env, username, extra = {}) {
  const id = crypto.randomUUID();
  await env.USERS.put(`user:${id}`, JSON.stringify({ id, username, public_key: `pk-${id}`, id_hash: `h-${id}`, ...extra }));
  await env.USERS.put(`username:${username}`, id);
  await env.USERS.put(`pubkey:pk-${id}`, id);
  await env.USERS.put(`idhash:h-${id}`, id);
  return id;
}

async function seedAdmin(env) {
  const id = await seedUser(env, 'root-admin');
  await env.USERS.put(`admin:${id}`, '1');
  return id;
}

async function upload(env, sessionId, { token } = {}) {
  const form = new FormData();
  form.set('side', 'front');
  form.set('id_type', 'Passport');
  form.set('session_id', sessionId);
  form.set('upload_token', uploadTokenOf(sessionId) || '0'.repeat(64));
  form.set('document', new File([JPEG], 'id', { type: 'image/jpeg' }));
  const headers = { 'cf-ipcountry': 'US' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${API}/api/kyc/upload`, { method: 'POST', headers, body: form }), env, ctx);
  return { status: res.status, json: await res.json() };
}

const VERIFIED_RECORD = {
  status: 'verified',
  extracted_dob: '1990-04-12',
  extracted_sex: 'F',
  ocr_name: 'Jane Q Applicant',
  age_verified: true,
  age_status: 'verified',
  minimum_age: 18,
  r2_key: 'kyc-us-session-x/front-1.bin',
  grok_analysis: { screening_reason: 'CRIMINAL-FLAG-SECRET', doc_decision: 'APPROVE', screening_flags: ['flag-secret'] },
};

// ── delete-user ────────────────────────────────────────────────────────────

test('delete-user refuses a caller with no token, and the account survives', async () => {
  const env = environment();
  const victim = await seedUser(env, 'victim');
  const res = await call(env, 'POST', '/api/admin/delete-user', { body: { username: 'victim' } });
  assert.equal(res.status, 403);
  assert.ok(await env.USERS.get(`user:${victim}`), 'the account must still exist');
  assert.equal(await env.USERS.get('username:victim'), victim);
});

test('delete-user refuses a signed-in user who is not an admin', async () => {
  const env = environment();
  const victim = await seedUser(env, 'victim');
  const mallory = await seedUser(env, 'mallory');
  const res = await call(env, 'POST', '/api/admin/delete-user', { token: mintToken(mallory), body: { username: 'victim' } });
  assert.equal(res.status, 403);
  assert.ok(await env.USERS.get(`user:${victim}`));
});

test('an admin can delete an account, every index goes with it, and the deletion is audited', async () => {
  const env = environment();
  const victim = await seedUser(env, 'victim');
  const admin = await seedAdmin(env);
  const res = await call(env, 'POST', '/api/admin/delete-user', { token: mintToken(admin), body: { username: 'victim' } });
  assert.equal(res.status, 200, res.text);
  assert.equal(await env.USERS.get(`user:${victim}`), null);
  assert.equal(await env.USERS.get('username:victim'), null);
  assert.equal(await env.USERS.get(`pubkey:pk-${victim}`), null);
  assert.equal(await env.USERS.get(`idhash:h-${victim}`), null);

  const audit = [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v));
  assert.equal(audit.length, 1);
  assert.equal(audit[0].action, 'DELETE_USER');
  assert.equal(audit[0].admin_id, admin);
  assert.equal(audit[0].target, 'victim');
});

// ── screening status ───────────────────────────────────────────────────────

test('screening status is for admins: no token and a non-admin token are both refused', async () => {
  const env = environment();
  const target = await seedUser(env, 'target', { risk_score: 71, risk_decision: 'REVIEW', screening_flags: ['flag-secret'] });
  const other = await seedUser(env, 'other');

  const anon = await call(env, 'GET', `/api/screening/status?user_id=${target}`);
  assert.equal(anon.status, 403);
  assert.ok(!anon.text.includes('flag-secret'));

  const nonAdmin = await call(env, 'GET', `/api/screening/status?user_id=${target}`, { token: mintToken(other) });
  assert.equal(nonAdmin.status, 403);
  assert.ok(!nonAdmin.text.includes('flag-secret'));

  const admin = await seedAdmin(env);
  const ok = await call(env, 'GET', `/api/screening/status?user_id=${target}`, { token: mintToken(admin) });
  assert.equal(ok.status, 200);
  assert.equal(ok.json.risk_score, 71);
});

// ── /api/kyc/status ────────────────────────────────────────────────────────

test('kyc status never serves a handoff record, which holds the applicant name and birthday', async () => {
  const env = environment();
  await env.KYC_STATUS.put('handoff:sess-1234567890', JSON.stringify({ full_name: 'Jane Q Applicant', birthday: '1990-04-12' }));
  const res = await call(env, 'GET', '/api/kyc/status/handoff:sess-1234567890');
  assert.equal(res.status, 404);
  assert.ok(!res.text.includes('Jane'));
  assert.ok(!res.text.includes('1990'));
});

test('kyc status does not read a registered account record by its user id', async () => {
  const env = environment();
  const id = await seedUser(env, 'victim');
  await env.KYC_STATUS.put(`kyc-${id}-front`, JSON.stringify({ ...VERIFIED_RECORD, status: 'linked' }));
  const res = await call(env, 'GET', `/api/kyc/status/kyc-${id}-front`);
  assert.equal(res.status, 404);
  assert.ok(!res.text.includes('1990'));
});

test('kyc status refuses a key that is not a session id, in any shape', async () => {
  const env = environment();
  await env.KYC_STATUS.put('kyc-sess-1234567890-front', JSON.stringify(VERIFIED_RECORD));
  for (const key of ['kyc-short-front', 'kyc-sess-1234567890-extra', 'sess-1234567890', '..%2Fx', 'kyc--front', '']) {
    const res = await call(env, 'GET', `/api/kyc/status/${key}`);
    assert.equal(res.status, 404, `${key} should be refused`);
  }
});

test('a session polling its own verdict gets the verdict and the age result, and no document field', async () => {
  const env = environment();
  await startKycSession(env, { session: 'sess-1234567890' });
  await env.KYC_STATUS.put('kyc-sess-1234567890-front', JSON.stringify(VERIFIED_RECORD));
  const res = await call(env, 'GET', '/api/kyc/status/kyc-sess-1234567890-front', { headers: { 'X-KYC-Token': uploadTokenOf('sess-1234567890') } });
  assert.equal(res.status, 200);
  assert.equal(res.json.status, 'verified');
  assert.equal(res.json.verified, true);
  assert.equal(res.json.age_verified, true);
  for (const secret of ['1990-04-12', 'Jane Q Applicant', 'CRIMINAL-FLAG-SECRET', 'flag-secret', 'kyc-us-session-x', 'extracted_dob', 'grok_analysis', 'r2_key']) {
    assert.ok(!res.text.includes(secret), `the response must not carry ${secret}`);
  }
});

test('a rejected session is told why in plain words, without the screening reason', async () => {
  const env = environment();
  await startKycSession(env, { session: 'sess-1234567890' });
  await env.KYC_STATUS.put('kyc-sess-1234567890-front', JSON.stringify({ ...VERIFIED_RECORD, status: 'rejected', age_verified: false, age_status: 'underage', minimum_age: 18 }));
  const res = await call(env, 'GET', '/api/kyc/status/kyc-sess-1234567890-front', { headers: { 'X-KYC-Token': uploadTokenOf('sess-1234567890') } });
  assert.equal(res.status, 200);
  assert.equal(res.json.status, 'rejected');
  assert.deepEqual(res.json.decision.reasons, ['You must be at least 18 years old']);
  assert.ok(!res.text.includes('CRIMINAL-FLAG-SECRET'));
});

// ── /api/kyc/session ───────────────────────────────────────────────────────

test('kyc session status answers for a registered account id exactly like a session nobody started', async () => {
  const env = environment();
  const id = await seedUser(env, 'victim');
  await env.KYC_STATUS.put(`kyc-${id}-front`, JSON.stringify({ ...VERIFIED_RECORD, status: 'linked' }));
  const res = await call(env, 'GET', `/api/kyc/session/${id}`);
  assert.equal(res.status, 404);
  const unknown = await call(env, 'GET', `/api/kyc/session/${crypto.randomUUID()}`);
  assert.deepEqual([unknown.status, unknown.json], [res.status, res.json]);
  assert.ok(!res.text.includes('Jane'));
});

test('kyc session status still reports a real registration session to the polling tab', async () => {
  const env = environment();
  await startKycSession(env, { session: 'sess-1234567890' });
  await env.KYC_STATUS.put('kyc-sess-1234567890-front', JSON.stringify(VERIFIED_RECORD));
  const res = await call(env, 'GET', '/api/kyc/session/sess-1234567890', { headers: { 'X-KYC-Token': uploadTokenOf('sess-1234567890') } });
  assert.equal(res.status, 200);
  assert.equal(res.json.status, 'verified');
  assert.equal(res.json.front_uploaded, true);
});

// ── /api/kyc/upload ────────────────────────────────────────────────────────

test('an upload cannot be filed under a registered account id, so the account record is left alone', async () => {
  const env = environment();
  const victim = await seedUser(env, 'victim');
  const original = JSON.stringify({ ...VERIFIED_RECORD, status: 'linked', user_id: victim });
  await env.KYC_STATUS.put(`kyc-${victim}-front`, original);
  const mallory = await seedUser(env, 'mallory');

  for (const token of [undefined, mintToken(mallory)]) {
    const res = await upload(env, victim, { token });
    assert.equal(res.status, 400, JSON.stringify(res.json));
    assert.equal(res.json.code, 'kyc_session_invalid');
  }
  assert.equal(await env.KYC_STATUS.get(`kyc-${victim}-front`), original, 'the victim record must be untouched');
  assert.equal(env.KYC_BUCKET.objects.size, 0, 'nothing may be written to the KYC bucket');
});

test('an upload with a session id that is not an id at all is refused', async () => {
  for (const bad of ['short', 'has spaces in it', '../../etc/passwd', 'x'.repeat(200)]) {
    const env = environment();
    const res = await upload(env, bad);
    assert.equal(res.status, 400, bad);
    assert.equal(env.KYC_BUCKET.objects.size, 0);
    assert.equal(env.KYC_STATUS.store.size, 0);
  }
});

test('a fresh registration session still uploads, and a retake replaces its own earlier capture', async () => {
  const env = environment();
  const { session } = await startKycSession(env);
  const first = await upload(env, session);
  assert.equal(first.status, 200, JSON.stringify(first.json));
  assert.equal(first.json.verification_id, `kyc-${session}-front`);
  const second = await upload(env, session);
  assert.equal(second.status, 200);
  assert.equal(JSON.parse(await env.KYC_STATUS.get(`kyc-${session}-front`)).status, 'uploaded');
});

// ── /api/kyc/submit ────────────────────────────────────────────────────────

test('submit cannot re-run verification on a registered account record', async () => {
  const env = environment();
  const victim = await seedUser(env, 'victim');
  const original = JSON.stringify({ ...VERIFIED_RECORD, status: 'linked', user_id: victim, r2_key: 'kyc-us-x/front.bin' });
  await env.KYC_STATUS.put(`kyc-${victim}-front`, original);
  const res = await call(env, 'POST', '/api/kyc/submit', { body: { session_id: victim, needs_back: false } });
  assert.equal(res.status, 400, res.text);
  assert.equal(res.json.code, 'kyc_session_invalid');
  assert.equal(await env.KYC_STATUS.get(`kyc-${victim}-front`), original);
});

test('submit stops after the hourly cap, before it pays for the model', async () => {
  const env = environment();
  const { session } = await startKycSession(env);
  await env.SCREENING.put('kyc-submit-rate:203.0.113.9', '20');
  const res = await call(env, 'POST', '/api/kyc/submit', {
    headers: { 'cf-connecting-ip': '203.0.113.9' },
    body: { session_id: session, upload_token: uploadTokenOf(session), needs_back: false },
  });
  assert.equal(res.status, 429);
  assert.equal(res.json.code, 'rate_limited');
});

test('submit still tells a session with no upload to upload first (the cap does not get in the way)', async () => {
  const env = environment();
  const { session, upload_token } = await startKycSession(env);
  const res = await call(env, 'POST', '/api/kyc/submit', { body: { session_id: session, upload_token, needs_back: false } });
  assert.equal(res.status, 400);
  assert.equal(res.json.status, 'incomplete');
});

// ── per-address limiter on the two KYC routes ──────────────────────────────

test('upload and submit are behind the per-address limiter', async () => {
  const used = new Map();
  const limiter = { async limit({ key }) { const n = (used.get(key) || 0) + 1; used.set(key, n); return { success: n <= 1 }; } };
  const env = environment({ AUTH_RATE_LIMITER: limiter });
  const headers = { 'CF-Connecting-IP': '198.51.100.7' };

  const first = await call(env, 'POST', '/api/kyc/submit', { headers, body: { session_id: crypto.randomUUID() } });
  assert.notEqual(first.status, 429);
  const second = await call(env, 'POST', '/api/kyc/submit', { headers, body: { session_id: crypto.randomUUID() } });
  assert.equal(second.status, 429);
  const third = await call(env, 'POST', '/api/kyc/upload', { headers, body: {} });
  assert.equal(third.status, 429);
});

// ── handoff ────────────────────────────────────────────────────────────────

test('a handoff link is only minted for an id that upload and submit will accept', async () => {
  const env = environment();
  const victim = await seedUser(env, 'victim');
  const reserved = await call(env, 'POST', '/api/kyc/handoff', { body: { session_id: victim, full_name: 'x', birthday: '1990-01-01' } });
  assert.equal(reserved.status, 400);
  assert.equal(env.KYC_STATUS.store.has(`handoff:${victim}`), false);

  const started = await startKycSession(env);
  const unstarted = await call(env, 'POST', '/api/kyc/handoff', { body: { session_id: crypto.randomUUID(), upload_token: started.upload_token, full_name: 'x', birthday: '1990-01-01' } });
  assert.equal(unstarted.status, 403, 'only a started session, with its own upload token');
  const ok = await call(env, 'POST', '/api/kyc/handoff', { body: { session_id: started.session, upload_token: started.upload_token, full_name: 'x', birthday: '1990-01-01' } });
  assert.equal(ok.status, 200, ok.text);
  // The phone link carries the upload token, in the fragment, and never the claim secret.
  assert.ok(ok.json.verify_url.endsWith(`#t=${started.upload_token}`), ok.json.verify_url);
  assert.ok(!ok.text.includes(started.claim_secret));
});
