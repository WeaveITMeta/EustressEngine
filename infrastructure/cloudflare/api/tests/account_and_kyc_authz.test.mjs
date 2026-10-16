// Who may read or change accounts and identity-verification records. Each route
// here once answered a caller it should have refused. The real Worker runs
// against in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeBucket, ctx } from './helpers/load_worker.mjs';
import { startKycSession, uploadTokenOf } from './helpers/kyc_session.mjs';

const worker = await loadWorker();

const ADMIN = '99999999-9999-4999-8999-999999999999';
const VICTIM = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const SESSION = 'session-1234567890abcdef';
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]);
const T = { admin: mintToken(ADMIN), victim: mintToken(VICTIM), other: mintToken(OTHER) };

async function environment() {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv(), SCREENING: fakeKv(), KYC_BUCKET: fakeBucket(), PAYOUTS: fakeKv() };
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${VICTIM}`, JSON.stringify({ kyc_verified: true, id: VICTIM, username: 'victim_user', public_key: 'pk-victim', id_hash: 'idh-victim', risk_score: 42, risk_decision: 'REVIEW', screening_flags: ['flag-a'], last_screened: '2026-09-01T00:00:00.000Z' }));
  await env.USERS.put('username:victim_user', VICTIM);
  await env.USERS.put('pubkey:pk-victim', VICTIM);
  await env.USERS.put('idhash:idh-victim', VICTIM);
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ kyc_verified: true, id: OTHER, username: 'other_user' }));
  // SESSION, started as /api/kyc/start would (its upload token: uploadTokenOf).
  await startKycSession(env, { session: SESSION });
  return env;
}

async function call(env, method, path, { token, body, form, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  let payload;
  if (form) payload = form;
  else if (body !== undefined) { payload = JSON.stringify(body); h['Content-Type'] = 'application/json'; }
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: payload }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

// ── Deleting an account ──────────────────────────────────────────────────────

test('deleting an account takes an admin: anonymous callers and ordinary accounts delete nothing', async () => {
  const env = await environment();
  for (const token of [undefined, T.other, T.victim]) {
    const r = await call(env, 'POST', '/api/admin/delete-user', { token, body: { username: 'victim_user' } });
    assert.ok([401, 403].includes(r.status), `got ${r.status}`);
    assert.notEqual(await env.USERS.get(`user:${VICTIM}`), null, 'the account is still there');
    assert.equal(await env.USERS.get('username:victim_user'), VICTIM);
    assert.equal(await env.USERS.get('pubkey:pk-victim'), VICTIM);
  }
});

test('an admin can delete an account, and the deletion is logged with who did it', async () => {
  const env = await environment();
  const r = await call(env, 'POST', '/api/admin/delete-user', { token: T.admin, body: { username: 'victim_user' } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(await env.USERS.get(`user:${VICTIM}`), null);
  assert.equal(await env.USERS.get('username:victim_user'), null);
  assert.equal(await env.USERS.get('pubkey:pk-victim'), null);
  assert.equal(await env.USERS.get('idhash:idh-victim'), null);
  const entries = [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v));
  assert.equal(entries.length, 1);
  assert.deepEqual([entries[0].action, entries[0].admin_id, entries[0].target], ['DELETE_USER', ADMIN, 'victim_user']);
  assert.equal((await call(env, 'POST', '/api/admin/delete-user', { token: T.admin, body: { username: 'nobody_here' } })).status, 404);
  assert.equal((await call(env, 'POST', '/api/admin/delete-user', { token: T.admin, body: {} })).status, 400);
});

// ── Reading a verification record ────────────────────────────────────────────

async function seedRecords(env) {
  await startKycSession(env, { session: 'rejectedsession01' });
  await startKycSession(env, { session: 'screenedsession01' });
  await env.KYC_STATUS.put(`kyc-${SESSION}-front`, JSON.stringify({
    status: 'verified', r2_key: 'kyc-us-session/front.bin', ocr_name: 'Ada Lovelace', extracted_dob: '1990-01-01', age_status: 'verified', age_verified: true, minimum_age: 18,
    grok_analysis: { doc_decision: 'APPROVE', screening_reason: 'private screening sentence', screening_flags: ['secret-flag'], risk_score: 77 },
  }));
  await env.KYC_STATUS.put('kyc-rejectedsession01-front', JSON.stringify({
    status: 'rejected', r2_key: 'k', age_status: 'underage', age_verified: false, minimum_age: 18,
    grok_analysis: { doc_decision: 'APPROVE', screening_decision: 'DENY', screening_reason: 'a private background-check sentence', risk_score: 90 },
  }));
  await env.KYC_STATUS.put('handoff:' + SESSION, JSON.stringify({ full_name: 'Ada Lovelace', birthday: '1990-01-01', username: 'ada' }));
  await env.KYC_STATUS.put(`kyc-${VICTIM}-front`, JSON.stringify({ status: 'linked', ocr_name: 'Victim Name', extracted_dob: '1980-05-05', age_verified: true }));
}

test('a verification status shows the applicant their outcome and nothing about how it was reached', async () => {
  const env = await environment();
  await seedRecords(env);
  const r = await call(env, 'GET', `/api/kyc/status/kyc-${SESSION}-front`, { headers: { 'X-KYC-Token': uploadTokenOf(SESSION) } });
  assert.equal(r.status, 200);
  assert.deepEqual([r.body.status, r.body.verified, r.body.age_verified, r.body.minimum_age], ['verified', true, true, 18]);
  const text = JSON.stringify(r.body);
  for (const secret of ['r2_key', 'kyc-us-session', 'extracted_dob', '1990-01-01', 'Ada', 'grok_analysis', 'secret-flag', 'private screening', 'risk_score', '77']) {
    assert.ok(!text.includes(secret), `the status leaks ${secret}: ${text}`);
  }
});

test('a rejection says why in plain words, and never repeats a background-check finding', async () => {
  const env = await environment();
  await seedRecords(env);
  const r = await call(env, 'GET', '/api/kyc/status/kyc-rejectedsession01-front', { headers: { 'X-KYC-Token': uploadTokenOf('rejectedsession01') } });
  assert.equal(r.body.status, 'rejected');
  assert.ok(Array.isArray(r.body.decision.reasons) && r.body.decision.reasons.length === 1);
  assert.match(r.body.decision.reasons[0], /18/);
  assert.ok(!JSON.stringify(r.body).includes('background-check'));

  await env.KYC_STATUS.put('kyc-screenedsession01-front', JSON.stringify({ status: 'rejected', age_status: 'verified', age_verified: true, grok_analysis: { doc_decision: 'APPROVE', screening_decision: 'DENY', screening_reason: 'a private background-check sentence' } }));
  const s = await call(env, 'GET', '/api/kyc/status/kyc-screenedsession01-front', { headers: { 'X-KYC-Token': uploadTokenOf('screenedsession01') } });
  assert.ok(!JSON.stringify(s.body).includes('background-check'));
  assert.match(s.body.decision.reasons[0], /requires review/i);
});

test('only a verification record is readable: no handoff record, no other key, no malformed id', async () => {
  const env = await environment();
  await seedRecords(env);
  for (const id of [`handoff:${SESSION}`, `handoff%3A${SESSION}`, `user:${VICTIM}`, 'kyc-short-front', `kyc-${SESSION}-other`, `kyc-${SESSION}-front%00`, `..%2Fkyc-${SESSION}-front`, `kyc-${SESSION}`, '']) {
    const r = await call(env, 'GET', `/api/kyc/status/${id}`);
    assert.equal(r.status, 404, id);
    assert.ok(!JSON.stringify(r.body).includes('Ada Lovelace'), id);
  }
});

test('an account\'s own verification record is not readable through the public status routes', async () => {
  const env = await environment();
  await seedRecords(env);
  for (const path of [`/api/kyc/status/kyc-${VICTIM}-front`, `/api/kyc/session/${VICTIM}`]) {
    const r = await call(env, 'GET', path);
    assert.ok(!JSON.stringify(r.body).includes('Victim Name'), `${path}: ${JSON.stringify(r.body)}`);
    assert.ok([200, 404].includes(r.status));
  }
  assert.equal((await call(env, 'GET', `/api/kyc/status/kyc-${VICTIM}-front`)).status, 404);
});

// ── Screening records ────────────────────────────────────────────────────────

test('screening status is moderation data: only an admin reads it', async () => {
  const env = await environment();
  const get = (query, token) => call(env, 'GET', `/api/screening/status?${query}`, { token });

  for (const token of [undefined, T.other, T.victim]) {
    for (const query of [`user_id=${VICTIM}`, 'id_hash=idh-victim']) {
      const r = await get(query, token);
      assert.ok([401, 403].includes(r.status), `${query} got ${r.status}`);
      assert.ok(!JSON.stringify(r.body).includes('flag-a'));
      assert.ok(!JSON.stringify(r.body).includes('42'));
    }
  }
  const admin = await get(`user_id=${VICTIM}`, T.admin);
  assert.equal(admin.status, 200);
  assert.deepEqual([admin.body.risk_score, admin.body.decision, admin.body.flags], [42, 'REVIEW', ['flag-a']]);
});

test('running a screening check, which spends a paid call and writes the record, takes an admin', async () => {
  const env = await environment();
  await env.SCREENING.put('screen:victim_user', JSON.stringify({ decision: 'DENY' }));
  for (const token of [undefined, T.other, T.victim]) {
    const r = await call(env, 'POST', '/api/screening/check', { token, body: { username: 'victim_user', birthday: '1990-01-01' } });
    assert.ok([401, 403].includes(r.status), `got ${r.status}`);
  }
  assert.deepEqual(JSON.parse(await env.SCREENING.get('screen:victim_user')), { decision: 'DENY' }, 'the stored result is untouched');
});

// ── Uploading and submitting identity documents ──────────────────────────────

function uploadForm(sessionId, side = 'front') {
  const form = new FormData();
  form.set('side', side);
  form.set('id_type', 'Passport');
  if (sessionId !== undefined) form.set('session_id', sessionId);
  if (uploadTokenOf(sessionId)) form.set('upload_token', uploadTokenOf(sessionId));
  form.set('document', new File([PNG], 'id.png', { type: 'image/png' }));
  return form;
}
const upload = (env, sessionId, opts = {}) => call(env, 'POST', '/api/kyc/upload', { form: uploadForm(sessionId, opts.side), token: opts.token, headers: { 'cf-ipcountry': 'US' } });

test('an upload cannot take over an account\'s verification record by naming its id as the session', async () => {
  const env = await environment();
  await seedRecords(env);
  const before = await env.KYC_STATUS.get(`kyc-${VICTIM}-front`);
  for (const token of [undefined, T.other]) {
    const r = await upload(env, VICTIM, { token });
    assert.equal(r.status, 400, JSON.stringify(r.body));
    assert.equal(r.body.code, 'kyc_session_invalid');
  }
  assert.equal(await env.KYC_STATUS.get(`kyc-${VICTIM}-front`), before, 'the record is untouched');
  assert.equal(env.KYC_BUCKET.objects.size, 0, 'nothing was stored');
});

test('a session is for registering: an account id is refused even for that account, and an ordinary session uploads', async () => {
  const env = await environment();
  assert.equal((await upload(env, VICTIM, { token: T.victim })).status, 400);
  const fresh = await startKycSession(env);
  assert.equal((await upload(env, fresh.session)).status, 200);
  assert.equal((await upload(env, undefined)).status, 400, 'no session id: only /api/kyc/start makes one');
  assert.equal((await upload(env, 'a-session-nobody-started-0001')).status, 403, 'a session nobody started');
});

test('a session id the Worker could mistake for another record is refused', async () => {
  const env = await environment();
  for (const bad of ['short', 'x'.repeat(101), 'has:colon-1234567', 'has space 1234567', 'a/b/c/d/e/f/g/h', 'handoff:12345678', '../../etc/xx']) {
    const r = await upload(env, bad);
    assert.equal(r.status, 400, bad);
  }
  assert.equal(env.KYC_BUCKET.objects.size, 0);
});

/// Counts calls to xAI and answers each with a plain approval.
async function withXai(fn) {
  const calls = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push(String(url));
    return new Response(JSON.stringify({ output: [{ type: 'message', content: [{ type: 'output_text', text: '{"doc_decision":"APPROVE","screening_decision":"APPROVE","risk_score":0,"extracted_dob":"1990-01-01","extracted_name":"Ada Lovelace"}' }] }] }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try { await fn(calls); } finally { globalThis.fetch = real; }
}

const submit = (env, sessionId, token) => call(env, 'POST', '/api/kyc/submit', { token, body: { session_id: sessionId, upload_token: uploadTokenOf(sessionId), full_name: 'Ada Lovelace', birthday: '1990-01-01' } });

test('a finished verification is returned as it is: submitting again makes no new paid call', async () => {
  const env = { ...(await environment()), GROK_API_KEY: 'xai-test-not-real' };
  await upload(env, SESSION);
  await upload(env, SESSION, { side: 'back' });
  await withXai(async (calls) => {
    const first = await submit(env, SESSION);
    assert.equal(first.status, 200, JSON.stringify(first.body));
    assert.equal(calls.length, 1);
    const again = await submit(env, SESSION);
    assert.equal(again.status, 200);
    assert.equal(again.body.status, first.body.status);
    assert.equal(calls.length, 1, 'no second paid call');
  });
});

test('submitting for an account\'s own id, or with a malformed session id, does nothing', async () => {
  const env = { ...(await environment()), GROK_API_KEY: 'xai-test-not-real' };
  await seedRecords(env);
  await withXai(async (calls) => {
    assert.equal((await submit(env, VICTIM)).status, 400);
    assert.equal((await submit(env, VICTIM, T.other)).status, 400);
    assert.equal((await submit(env, 'bad:id-123456')).status, 400);
    assert.equal(calls.length, 0);
  });
});

test('one session can ask for only a few paid checks in a day', async () => {
  const env = { ...(await environment()), GROK_API_KEY: 'xai-test-not-real' };
  await withXai(async (calls) => {
    let last;
    for (let i = 0; i < 8; i++) {
      await upload(env, SESSION);
      await upload(env, SESSION, { side: 'back' });
      last = await submit(env, SESSION);
      // Put the record back to uploaded, as a rejected applicant's retry does.
      const rec = JSON.parse(await env.KYC_STATUS.get(`kyc-${SESSION}-front`));
      rec.status = 'rejected';
      await env.KYC_STATUS.put(`kyc-${SESSION}-front`, JSON.stringify(rec));
    }
    assert.equal(last.status, 429);
    assert.ok(calls.length <= 5, `${calls.length} paid calls`);
  });
});

test('the document routes are limited per address, like the auth routes', async () => {
  let used = 0;
  const env = { ...(await environment()), AUTH_RATE_LIMITER: { limit: async () => ({ success: ++used <= 2 }) } };
  const ip = { 'CF-Connecting-IP': '203.0.113.7' };
  const hit = (path) => (path === '/api/kyc/upload'
    ? call(env, 'POST', path, { headers: ip, form: uploadForm('limit-test-session-1') })
    : call(env, 'POST', path, { headers: ip, body: {} }));
  for (const path of ['/api/kyc/upload', '/api/kyc/submit', '/api/kyc/handoff']) {
    used = 0;
    await hit(path); await hit(path);
    assert.equal((await hit(path)).status, 429, path);
  }
});

// ── A play count ─────────────────────────────────────────────────────────────

test('a play is counted for a signed-in account and the author is read from the listing, never from the request', async () => {
  const env = await environment();
  const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, author_id: OTHER, is_public: true, moderation: { status: 'approved', rating: 'all_ages' } }));
  const anon = await call(env, 'POST', '/api/social/play', { body: { simulation_id: SIM, author_id: VICTIM } });
  assert.equal(anon.status, 401);
  assert.equal(await env.SOCIAL.get(`plays:${SIM}`), null);
  assert.equal(await env.SOCIAL.get(`totalPlays:${VICTIM}`), null, 'a named author is not credited');

  const r = await call(env, 'POST', '/api/social/play', { token: T.victim, body: { simulation_id: SIM, author_id: VICTIM } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(await env.SOCIAL.get(`totalPlays:${VICTIM}`), null, 'the request\'s author_id is ignored');
  assert.equal(await env.SOCIAL.get(`totalPlays:${OTHER}`), '1', 'the listing\'s author is credited');

  const again = await call(env, 'POST', '/api/social/play', { token: T.victim, body: { simulation_id: SIM } });
  assert.equal(again.status, 200);
  assert.equal(await env.SOCIAL.get(`plays:${SIM}`), '1', 'one account counts once a day');
});
