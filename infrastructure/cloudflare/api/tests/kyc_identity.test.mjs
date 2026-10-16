// Identity verification (src/kyc_identity.mjs), through the real Worker routes.
//
// Defect 1: when xAI refused the call (credits ran out: 402, 403, 429) the
// applicant was rejected with "Date of birth could not be read", their try was
// spent and nobody was told. An outage is now not a verdict.
//
// Defect 2: registration took id_hash from the request and needed no KYC
// session, so any 64-hex string showed as "Verified". Every account is now made
// from one verified session, and the identity facts come from its record.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeBucket, ctx } from './helpers/load_worker.mjs';
import { seedVerifiedKyc, startKycSession, claimSecretOf, uploadTokenOf, TEST_KYC_ID_PEPPER } from './helpers/kyc_session.mjs';

const worker = await loadWorker();

const ADMIN = '99999999-9999-4999-8999-999999999999';
const SESSION = 'kyc-session-abcdef0123';
const IP = '203.0.113.5';
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]);
const UNAVAILABLE = 'ID verification is temporarily unavailable. Your upload is saved; try again in a few minutes.';

function environment(extra = {}) {
  return {
    ...fakeEnv(), KYC_STATUS: fakeKv(), SCREENING: fakeKv(), KYC_BUCKET: fakeBucket(), PAYOUTS: fakeKv(), CHALLENGES: fakeKv(),
    GROK_API_KEY: 'xai-test-not-real-0123456789', KYC_ID_PEPPER: TEST_KYC_ID_PEPPER, ...extra,
  };
}

async function call(env, method, path, { token, body, form, headers } = {}) {
  const h = { 'cf-connecting-ip': IP, ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  let payload;
  if (form) payload = form;
  else if (body !== undefined) { payload = JSON.stringify(body); h['Content-Type'] = 'application/json'; }
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: payload }), env, ctx);
  return { status: res.status, headers: res.headers, body: await res.json().catch(() => null) };
}

/// Upload tokens of sessions a test started through the route; the rest come from the helper.
const uploadTokens = new Map();
const tokenFor = (sessionId) => uploadTokens.get(sessionId) || uploadTokenOf(sessionId);

function uploadForm(sessionId, side) {
  const form = new FormData();
  form.set('side', side);
  form.set('id_type', 'Passport');
  form.set('session_id', sessionId);
  form.set('upload_token', tokenFor(sessionId) || '');
  form.set('document', new File([PNG], 'id.png', { type: 'image/png' }));
  return form;
}
async function uploadBoth(env, sessionId = SESSION) {
  // A session this environment has not started yet is started, as /api/kyc/start would.
  if (!(await env.KYC_STATUS.get(`kycsess:${sessionId}`))) { uploadTokens.delete(sessionId); await startKycSession(env, { session: sessionId }); }
  for (const side of ['front', 'back']) {
    const r = await call(env, 'POST', '/api/kyc/upload', { form: uploadForm(sessionId, side), headers: { 'cf-ipcountry': 'US' } });
    assert.equal(r.status, 200, JSON.stringify(r.body));
  }
}
const submit = (env, sessionId = SESSION) => call(env, 'POST', '/api/kyc/submit', { body: { session_id: sessionId, upload_token: tokenFor(sessionId), full_name: 'Ada Lovelace', birthday: '1990-01-01' } });
const front = async (env, sessionId = SESSION) => JSON.parse(await env.KYC_STATUS.get(`kyc-${sessionId}-front`));

const xaiText = (text) => new Response(JSON.stringify({ output: [{ type: 'message', content: [{ type: 'output_text', text }] }] }), { status: 200, headers: { 'Content-Type': 'application/json' } });
const APPROVE = '{"doc_decision":"APPROVE","document_type":"passport","screening_decision":"APPROVE","risk_score":0,"extracted_dob":"1990-01-01","extracted_name":"Ada Lovelace"}';
const DENY = '{"doc_decision":"DENY","doc_reason":"This is a photo of a screen","screening_decision":"APPROVE","risk_score":0,"extracted_dob":"1990-01-01","extracted_name":"Ada Lovelace"}';
const CREDITS_429 = () => new Response(JSON.stringify({ code: 'credits exhausted', error: 'Your team has run out of credits. Key xai-ABCDEF0123456789 used.' }), { status: 429 });

/// Answers every xAI call with `answer()`, and counts the calls.
async function withXai(answer, fn) {
  const calls = [];
  const real = globalThis.fetch;
  globalThis.fetch = async (url, init) => { calls.push(String(url)); return answer(); };
  try { return await fn(calls); } finally { globalThis.fetch = real; }
}

const tries = async (env, sessionId = SESSION) => parseInt((await env.KYC_STATUS.get(`kyc-submits:${sessionId}`)) || '0', 10);
const ipCount = async (env) => parseInt((await env.SCREENING.get(`kyc-submit-rate:${IP}`)) || '0', 10);

// ── Defect 1: an outage is not a verdict ─────────────────────────────────────

test('xAI out of credits (429): 503 kyc_unavailable, no rejected record, no try spent, and the same uploads verify on retry', async () => {
  const env = environment();
  await uploadBoth(env);
  await withXai(CREDITS_429, async (calls) => {
    const r = await submit(env);
    assert.equal(calls.length, 1);
    assert.equal(r.status, 503, JSON.stringify(r.body));
    assert.deepEqual(r.body, { status: 'unavailable', code: 'kyc_unavailable', error: UNAVAILABLE });
    assert.ok(Number(r.headers.get('Retry-After')) > 0, 'a Retry-After header');
  });
  const rec = await front(env);
  assert.equal(rec.status, 'uploaded', 'the upload stays pending, not rejected');
  assert.equal(rec.grok_analysis, undefined);
  assert.equal(await tries(env), 0, 'the session try was given back');
  assert.equal(await ipCount(env), 0, 'the address count was given back');
  assert.equal(await env.SCREENING.get(`screen:session-${SESSION}`), null, 'no screening record');

  await withXai(() => xaiText(APPROVE), async () => {
    const again = await submit(env);
    assert.equal(again.status, 200, JSON.stringify(again.body));
    assert.equal(again.body.status, 'verified');
  });
  assert.equal(await tries(env), 1, 'a real verdict spends its try');
});

test('a parsed DENY still rejects, is stored, and spends the try', async () => {
  const env = environment();
  await uploadBoth(env);
  await withXai(() => xaiText(DENY), async () => {
    const r = await submit(env);
    assert.equal(r.status, 200, JSON.stringify(r.body));
    assert.equal(r.body.status, 'rejected');
    assert.equal(r.body.reason, 'This is a photo of a screen');
  });
  assert.equal((await front(env)).status, 'rejected');
  assert.equal(await tries(env), 1);
});

test('nothing verifies without a parsed APPROVE: an unreadable answer, or one with no verdict, is an outage', async () => {
  for (const text of ['I am unable to help with that request.', '{"extracted_dob":"1990-01-01","extracted_name":"Ada Lovelace"}', '{"doc_decision":"approve!"}', '{"doc_decision": APPROVE}']) {
    const env = environment();
    await uploadBoth(env);
    await withXai(() => xaiText(text), async () => {
      const r = await submit(env);
      assert.equal(r.status, 503, `${text}: ${JSON.stringify(r.body)}`);
      assert.equal(r.body.code, 'kyc_unavailable');
    });
    const rec = await front(env);
    assert.notEqual(rec.status, 'verified', text);
    assert.notEqual(rec.age_verified, true, text);
  }
});

test('a missing GROK_API_KEY, a 500 and a thrown fetch are outages too, never rejections', async () => {
  const cases = [
    ['no key', { GROK_API_KEY: '' }, () => xaiText(APPROVE)],
    ['a 500', {}, () => new Response('upstream', { status: 500 })],
    ['a throw', {}, () => { throw new Error('network down'); }],
  ];
  for (const [label, extra, answer] of cases) {
    const env = environment(extra);
    await uploadBoth(env);
    await withXai(answer, async () => {
      const r = await submit(env);
      assert.equal(r.status, 503, `${label}: ${JSON.stringify(r.body)}`);
    });
    assert.equal((await front(env)).status, 'uploaded', label);
    assert.equal(await tries(env), 0, label);
  }
});

test('the administrators are emailed once an hour while xAI refuses, not once per attempt, with no applicant data', async () => {
  const sent = [];
  const env = environment({ EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: class { constructor(from, to, raw) { this.to = to; this.raw = raw; } } });
  await uploadBoth(env);
  await withXai(CREDITS_429, async () => {
    for (let i = 0; i < 3; i++) assert.equal((await submit(env)).status, 503);
  });
  assert.equal(sent.length, 1, 'one email for three refusals in the hour');
  const subject = /^Subject: (.*)$/m.exec(sent[0].raw)[1];
  assert.match(subject, /^Grok refused KYC: 429 Your team has run out of credits/);
  assert.ok(!/ABCDEF0123456789/.test(sent[0].raw), 'no key text');
  const decoded = sent[0].raw.split('\r\n\r\n').map((p) => { try { return Buffer.from(p.replace(/\r\n/g, ''), 'base64').toString('utf8'); } catch { return ''; } }).join('\n');
  for (const applicant of ['Ada', 'Lovelace', '1990-01-01', SESSION]) assert.ok(!sent[0].raw.includes(applicant) && !decoded.includes(applicant), `no ${applicant}`);

  // The hour passes (the marker expires): the next refusal is told again.
  await env.SCREENING.delete('kyc:grok-alert');
  await withXai(CREDITS_429, async () => { await submit(env); });
  assert.equal(sent.length, 2);

  const health = await call(env, 'GET', '/health');
  assert.equal(health.body.integrations.grok.kyc.status, 'failing');
  assert.equal(health.body.integrations.grok.kyc.last_failure_status, 429);
  assert.ok(Date.parse(health.body.integrations.grok.kyc.last_failure_at) > 0);
  assert.ok(!JSON.stringify(health.body).includes('xai-test-not-real'), 'no key on /health');
  assert.ok(!JSON.stringify(health.body).includes('credits'), 'no xAI error text on /health');

  await withXai(() => xaiText(APPROVE), async () => { await submit(env); });
  assert.equal((await call(env, 'GET', '/health')).body.integrations.grok.kyc.status, 'ok');
});

test('a session rejected for a service error before this change runs the check again, with its spent tries back', async () => {
  const env = environment();
  await uploadBoth(env);
  // What the old code stored when xAI answered 429: a rejection that blamed the document.
  const rec = await front(env);
  await env.KYC_STATUS.put(`kyc-${SESSION}-front`, JSON.stringify({
    ...rec, status: 'rejected', age_status: 'unreadable', age_verified: false, extracted_dob: '',
    grok_analysis: { doc_decision: 'DENY', doc_reason: 'Verification service returned 429', screening_flags: ['verification_error'], requires_manual_review: true },
  }));
  await env.KYC_STATUS.put(`kyc-submits:${SESSION}`, '5');
  const status = await call(env, 'GET', `/api/kyc/status/kyc-${SESSION}-front`, { headers: { 'X-KYC-Token': tokenFor(SESSION) } });
  assert.deepEqual(status.body.decision.reasons, [UNAVAILABLE], 'the applicant is not told their ID was unreadable');

  await withXai(() => xaiText(APPROVE), async (calls) => {
    const r = await submit(env);
    assert.equal(calls.length, 1, 'the check ran again');
    assert.equal(r.status, 200, JSON.stringify(r.body));
    assert.equal(r.body.status, 'verified');
  });
});

test('a rejection the model gave is not reopened by a flag it chose itself', async () => {
  const env = environment();
  await uploadBoth(env);
  await withXai(() => xaiText('{"doc_decision":"DENY","screening_decision":"APPROVE","screening_flags":["verification_error"],"extracted_dob":"1990-01-01"}'), async () => {
    assert.equal((await submit(env)).body.status, 'rejected');
  });
  assert.deepEqual((await front(env)).grok_analysis.screening_flags, []);
});

// ── Defect 2: every account is made from one verified session ────────────────

const keyHex = () => crypto.randomBytes(32).toString('hex');
// A registration that names a session carries that session's claim secret, unless the test says otherwise.
const registerBody = (n, extra = {}) => ({
  username: `kyc_user_${n}`, public_key: keyHex(), birthday: '1990-01-01',
  ...(extra.kyc_session_id && !('kyc_claim_secret' in extra) ? { kyc_claim_secret: claimSecretOf(extra.kyc_session_id) } : {}),
  ...extra,
});
const userKeys = (env) => [...env.USERS.store.keys()].filter((k) => k.startsWith('user:'));

test('registration with no KYC session is refused with kyc_required, writes nothing, and calls no model', async () => {
  const env = environment();
  const before = [...env.USERS.store.keys()].length;
  await withXai(() => xaiText(APPROVE), async (calls) => {
    for (const extra of [{}, { id_hash: 'ab'.repeat(32), id_type: 'passport' }, { kyc_session_id: '' }, { kyc_session_id: 'kyc-session-never-made' }]) {
      const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, extra) });
      assert.equal(r.status, 403, JSON.stringify(r.body));
      assert.equal(r.body.code, 'kyc_required');
    }
    assert.equal(calls.length, 0, 'no screening call for an unauthenticated request');
  });
  assert.equal([...env.USERS.store.keys()].length, before, 'nothing written');
  assert.equal([...env.SCREENING.store.keys()].filter((k) => k.startsWith('screen:')).length, 0);
});

test('a session that is not verified, not of age, or more than 24 hours old is refused', async () => {
  const env = environment();
  const day = 24 * 3600 * 1000;
  const sessions = [
    await seedVerifiedKyc(env, { record: { status: 'uploaded' } }),
    await seedVerifiedKyc(env, { record: { status: 'rejected' } }),
    await seedVerifiedKyc(env, { record: { age_verified: false } }),
    await seedVerifiedKyc(env, { record: { status: 'linked' } }),
    await seedVerifiedKyc(env, { verified_at: new Date(Date.now() - day - 60_000).toISOString() }),
  ];
  for (const [i, kyc_session_id] of sessions.entries()) {
    const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(i, { kyc_session_id }) });
    assert.equal(r.status, 403, `${i}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'kyc_required');
  }
  assert.equal(userKeys(env).length, 0);
  // The control: a fresh verified session makes an account.
  const ok = await call(env, 'POST', '/api/auth/register', { body: registerBody(9, { kyc_session_id: await seedVerifiedKyc(env, { verified_at: new Date(Date.now() - day + 60_000).toISOString() }) }) });
  assert.equal(ok.status, 200, JSON.stringify(ok.body));
});

test('one session used twice in parallel makes one account, and a later use is refused too', async () => {
  const env = environment();
  const kyc_session_id = await seedVerifiedKyc(env);
  // The worst interleaving, as separate isolates can run it: all three requests
  // pass every read-only check before any of them writes a claim. Only the
  // claims (the person's, then the session's) can then tell them apart.
  const PARALLEL = 3;
  const realPut = env.KYC_BUCKET.put.bind(env.KYC_BUCKET);
  const held = [];
  env.KYC_BUCKET.put = async (key, ...rest) => {
    if (key.startsWith('claims/') && held.length < PARALLEL) {
      const go = new Promise((resolve) => held.push(resolve));
      if (held.length === PARALLEL) held.forEach((resolve) => resolve());
      await go;
    }
    return realPut(key, ...rest);
  };
  const results = await Promise.all([1, 2, 3].map((n) => call(env, 'POST', '/api/auth/register', { body: registerBody(n, { kyc_session_id }) })));
  env.KYC_BUCKET.put = realPut;
  assert.equal(held.length, PARALLEL, 'all three reached the claims before any wrote');
  assert.equal(results.filter((r) => r.status === 200).length, 1, JSON.stringify(results.map((r) => r.body)));
  assert.equal(userKeys(env).length, 1, 'one account');
  for (const r of results.filter((x) => x.status !== 200)) {
    assert.equal(r.status, 409, JSON.stringify(r.body));
    assert.ok(['kyc_id_in_use', 'kyc_session_used'].includes(r.body.code), JSON.stringify(r.body));
  }
  const later = await call(env, 'POST', '/api/auth/register', { body: registerBody(4, { kyc_session_id }) });
  assert.notEqual(later.status, 200);
  assert.equal(userKeys(env).length, 1);
});

test('id_hash, id_type and the date of birth come from the KYC record; the body\'s are ignored', async () => {
  const env = environment();
  const kyc_session_id = await seedVerifiedKyc(env, { name: 'Grace Brewster Hopper', dob: '1986-12-09' });
  const bodyHash = 'ff'.repeat(32);
  const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id, id_hash: bodyHash, id_type: 'library_card', birthday: '2001-02-03' }) });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const user = JSON.parse(await env.USERS.get(`user:${r.body.user.id}`));
  assert.match(user.id_hash, /^[0-9a-f]{64}$/);
  assert.notEqual(user.id_hash, bodyHash);
  assert.equal(await env.USERS.get(`idhash:${bodyHash}`), null, 'the body\'s hash indexes nothing');
  assert.equal(await env.USERS.get(`idhash:${user.id_hash}`), user.id);
  assert.equal(user.id_type, 'passport');
  assert.equal(user.birthday, '1986-12-09');
  assert.equal(user.kyc_verified, true);
  assert.equal(await env.KYC_STATUS.get(`kyc-${kyc_session_id}-front`), null, 'the session moved to the account');
  assert.equal(JSON.parse(await env.KYC_STATUS.get(`kyc-${user.id}-front`)).status, 'linked');

  // The same person (the name in another order and case, the same date of birth) with a second session: one account.
  const again = await call(env, 'POST', '/api/auth/register', { body: registerBody(2, { kyc_session_id: await seedVerifiedKyc(env, { name: 'HOPPER, Grace Brewster', dob: '1986-12-09' }), id_hash: 'ee'.repeat(32) }) });
  assert.equal(again.status, 409, JSON.stringify(again.body));
  assert.equal(again.body.code, 'kyc_id_in_use');
});

test('an id_hash without kyc_verified shows no Verified badge; a KYC verification does', async () => {
  const env = environment();
  const LEGACY = '33333333-3333-4333-8333-333333333333';
  const REAL = '44444444-4444-4444-8444-444444444444';
  await env.USERS.put(`user:${LEGACY}`, JSON.stringify({ id: LEGACY, username: 'legacy_hash', id_hash: 'ab'.repeat(32), created_at: '2026-01-01T00:00:00.000Z' }));
  await env.USERS.put('username:legacy_hash', LEGACY);
  await env.USERS.put(`user:${REAL}`, JSON.stringify({ id: REAL, username: 'legacy_real', id_hash: 'cd'.repeat(32), kyc_verified: true, created_at: '2026-01-01T00:00:00.000Z' }));
  await env.USERS.put('username:legacy_real', REAL);
  const hashOnly = await call(env, 'GET', '/api/community/users/legacy_hash');
  assert.equal(hashOnly.status, 200, JSON.stringify(hashOnly.body));
  assert.equal(hashOnly.body.is_verified, false);
  assert.ok(!hashOnly.body.badges.some((b) => b.id === 'verified'));
  const verified = await call(env, 'GET', '/api/community/users/legacy_real');
  assert.equal(verified.body.is_verified, true);
  assert.ok(verified.body.badges.some((b) => b.id === 'verified'));
  const search = await call(env, 'GET', '/api/community/search?q=legacy');
  const byName = Object.fromEntries(search.body.users.map((u) => [u.username, u.is_verified]));
  assert.deepEqual(byName, { legacy_hash: false, legacy_real: true });
});

// ── Accounts made without KYC: gated at sign-in, never deleted ───────────────

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}
async function signIn(env, key) {
  const challenge = await call(env, 'POST', '/api/auth/challenge', { body: { public_key: key.hex } });
  assert.equal(challenge.status, 200, JSON.stringify(challenge.body));
  return call(env, 'POST', '/api/auth/verify-challenge', { body: { format: 'eustress-login-v1', public_key: key.hex, challenge: challenge.body.challenge, signature: key.sign(['eustress-login-v1', 'https://api.eustress.dev', challenge.body.challenge].join('\n')) } });
}

test('an account made without KYC signs in and reads its own account, is refused everything else, is not deleted, and verifies with a session', async () => {
  const env = environment();
  const OLD = '55555555-5555-4555-8555-555555555555';
  const FRIEND = '66666666-6666-4666-8666-666666666666';
  const key = keyPair();
  await env.USERS.put(`user:${OLD}`, JSON.stringify({ id: OLD, username: 'old_timer', public_key: key.hex, id_hash: 'ab'.repeat(32), birthday: '1990-01-01', created_at: '2026-01-01T00:00:00.000Z' }));
  await env.USERS.put('username:old_timer', OLD);
  await env.USERS.put(`pubkey:${key.hex}`, OLD);
  await env.USERS.put(`user:${FRIEND}`, JSON.stringify({ id: FRIEND, username: 'friend', kyc_verified: true }));
  await env.USERS.put('username:friend', FRIEND);

  const signed = await signIn(env, key);
  assert.equal(signed.status, 200, JSON.stringify(signed.body));
  assert.equal(signed.body.user.kyc_required, true, 'sign-in says verification is required');
  const token = signed.body.token;

  const me = await call(env, 'GET', '/api/auth/me', { token });
  assert.equal(me.status, 200, 'reading its own account works');
  assert.equal(me.body.kyc_required, true);
  const follow = await call(env, 'POST', '/api/social/follow', { token, body: { username: 'friend' } });
  assert.equal(follow.status, 403, JSON.stringify(follow.body));
  assert.equal(follow.body.code, 'kyc_required');
  assert.equal(await env.SOCIAL.get(`following:${OLD}`), null, 'nothing done');
  const zone = await call(env, 'PUT', '/api/account/time-zone', { token, body: { time_zone: 'America/Phoenix' } });
  assert.equal(zone.status, 403);
  // Its commerce API key is refused the same way.
  const apiKey = `ek_test_${'a'.repeat(40)}`;
  await env.USERS.put(`apikey:${crypto.createHash('sha256').update(apiKey).digest('hex')}`, JSON.stringify({ account_id: OLD, mode: 'test', key_type: 'commerce', key_id: 'key_1' }));
  const viaKey = await call(env, 'POST', '/api/commerce/products', { token: apiKey, body: { name: 'Thing', price: 5 } });
  assert.equal(viaKey.status, 403, JSON.stringify(viaKey.body));
  assert.equal(viaKey.body.code, 'kyc_required');
  assert.notEqual(await env.USERS.get(`user:${OLD}`), null, 'the account is still there');

  // An administrator counts them first, writing nothing.
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'root' }));
  const before = await env.USERS.get(`user:${OLD}`);
  const count = await call(env, 'GET', '/api/admin/kyc-legacy', { token: mintToken(ADMIN) });
  assert.equal(count.status, 200, JSON.stringify(count.body));
  assert.equal(count.body.dry_run, true);
  const c = count.body.counts;
  assert.deepEqual({ gated: c.gated, with_hash: c.gated_with_id_hash, admins: c.gated_admins, verified: c.verified }, { gated: 2, with_hash: 1, admins: 1, verified: 0 });
  assert.equal(count.body.counts.flag_without_record, 1, 'friend says kyc_verified with no KYC record behind it');
  assert.deepEqual(count.body.gated_admin_ids, [ADMIN], 'the gated administrators, by id');
  assert.equal(await env.USERS.get(`user:${OLD}`), before, 'a count writes nothing');

  // The account verifies: a verified session linked to it, and the gate opens.
  const kyc_session_id = await seedVerifiedKyc(env, { name: 'Old Timer', dob: '1990-01-01' });
  const linked = await call(env, 'POST', '/api/kyc/link', { token, body: { kyc_session_id, kyc_claim_secret: claimSecretOf(kyc_session_id) } });
  assert.equal(linked.status, 200, JSON.stringify(linked.body));
  const user = JSON.parse(await env.USERS.get(`user:${OLD}`));
  assert.equal(user.kyc_verified, true);
  assert.notEqual(user.id_hash, 'ab'.repeat(32), 'the id_hash is the server\'s now');
  assert.equal(await env.USERS.get(`idhash:${user.id_hash}`), OLD);
  assert.equal((await call(env, 'POST', '/api/social/follow', { token, body: { username: 'friend' } })).status, 200);
  assert.equal((await call(env, 'POST', '/api/kyc/link', { token: mintToken(FRIEND), body: { kyc_session_id, kyc_claim_secret: claimSecretOf(kyc_session_id) } })).status, 200, 'friend is already verified: nothing to link');
});

test('an administrator is not locked out by the gate, and logout is always allowed', async () => {
  const env = environment();
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'root' }));
  const r = await call(env, 'POST', '/api/admin/kyc-legacy', { token: mintToken(ADMIN), body: {} });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.dry_run, true, 'a POST without apply: true is a dry run');
  const OLD = '77777777-7777-4777-8777-777777777777';
  await env.USERS.put(`user:${OLD}`, JSON.stringify({ id: OLD, username: 'old_two' }));
  assert.equal((await call(env, 'POST', '/api/auth/logout', { token: mintToken(OLD) })).status, 200);
});

test('the reconcile sets kyc_verified from the KYC record only, and only when asked', async () => {
  const env = environment();
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'root', kyc_verified: true }));
  await env.KYC_STATUS.put(`kyc-${ADMIN}-front`, JSON.stringify({ status: 'linked' }));
  const BACKED = '88888888-8888-4888-8888-888888888888';
  const BARE = 'aaaaaaaa-8888-4888-8888-888888888888';
  await env.USERS.put(`user:${BACKED}`, JSON.stringify({ id: BACKED, username: 'backed' }));
  await env.KYC_STATUS.put(`kyc-${BACKED}-front`, JSON.stringify({ status: 'linked' }));
  await env.USERS.put(`user:${BARE}`, JSON.stringify({ id: BARE, username: 'bare', kyc_verified: true, id_hash: 'ab'.repeat(32) }));
  const dry = await call(env, 'POST', '/api/admin/kyc-legacy', { token: mintToken(ADMIN), body: { apply: false } });
  assert.deepEqual({ open: dry.body.would_open, gate: dry.body.would_gate, changed: dry.body.changed, dry: dry.body.dry_run }, { open: 1, gate: 1, changed: 0, dry: true });
  // apply: true sets kyc_verified from false to true only.
  const applied = await call(env, 'POST', '/api/admin/kyc-legacy', { token: mintToken(ADMIN), body: { apply: true } });
  assert.equal(applied.body.changed, 1);
  assert.equal(JSON.parse(await env.USERS.get(`user:${BACKED}`)).kyc_verified, true);
  assert.equal(JSON.parse(await env.USERS.get(`user:${BARE}`)).kyc_verified, true, 'apply never takes a flag away');
  // Taking it away is its own, explicit request.
  const revoked = await call(env, 'POST', '/api/admin/kyc-legacy', { token: mintToken(ADMIN), body: { revoke_unbacked: true } });
  assert.equal(revoked.body.changed, 1);
  assert.equal(JSON.parse(await env.USERS.get(`user:${BARE}`)).kyc_verified, false);
  assert.notEqual(await env.USERS.get(`user:${BARE}`), null, 'never deleted');
});

// ── Round 2: the claim secret, the pepper, the person claimed in R2 ─────────

const sha256 = (text) => crypto.createHash('sha256').update(text).digest('hex');
const hmac = (key, text) => crypto.createHmac('sha256', key).update(text).digest('hex');

test('a started session hands out its secrets once and stores only their SHA-256', async () => {
  const env = environment();
  const r = await call(env, 'POST', '/api/kyc/start');
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const { session_id, claim_secret, upload_token } = r.body;
  assert.match(session_id, /^[0-9a-f-]{36}$/, 'a server-chosen id');
  assert.match(claim_secret, /^[0-9a-f]{64}$/, '256 bits');
  assert.match(upload_token, /^[0-9a-f]{64}$/, '256 bits');
  assert.notEqual(claim_secret, upload_token);
  assert.equal(r.headers.get('Cache-Control'), 'no-store');
  const stored = JSON.parse(await env.KYC_STATUS.get(`kycsess:${session_id}`));
  assert.deepEqual({ claim: stored.claim_hash, upload: stored.upload_hash }, { claim: sha256(claim_secret), upload: sha256(upload_token) });
  assert.notEqual((await call(env, 'POST', '/api/kyc/start')).body.session_id, session_id, 'each start is a new session');
});

test('the right session id with a wrong or missing claim secret is refused, for registering and for linking', async () => {
  const env = environment();
  const session = await seedVerifiedKyc(env);
  const OLD = '12121212-1212-4212-8212-121212121212';
  await env.USERS.put(`user:${OLD}`, JSON.stringify({ id: OLD, username: 'old_one' }));
  for (const kyc_claim_secret of [undefined, '', 'ab'.repeat(32), claimSecretOf(session).slice(0, 63), claimSecretOf(session).toUpperCase(), uploadTokenOf(session)]) {
    const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: session, kyc_claim_secret }) });
    assert.equal(r.status, 403, `${kyc_claim_secret}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'kyc_required');
    const l = await call(env, 'POST', '/api/kyc/link', { token: mintToken(OLD), body: { kyc_session_id: session, kyc_claim_secret } });
    assert.equal(l.status, 403, `link ${kyc_claim_secret}: ${JSON.stringify(l.body)}`);
  }
  assert.equal(userKeys(env).length, 1, 'no account was made');
  assert.equal(JSON.parse(await env.USERS.get(`user:${OLD}`)).kyc_verified, undefined, 'nothing was linked');
  assert.equal([...env.KYC_BUCKET.objects.keys()].filter((k) => k.startsWith('claims/')).length, 0, 'nothing was claimed');
  // The control: the claim secret registers.
  assert.equal((await call(env, 'POST', '/api/auth/register', { body: registerBody(2, { kyc_session_id: session }) })).status, 200);
});

test('upload, submit and the hand-off need the session\'s upload token; the claim secret is not one', async () => {
  const env = environment();
  const { session, claim_secret, upload_token } = await startKycSession(env);
  const form = (token) => { const f = uploadForm(session, 'front'); if (token === undefined) f.delete('upload_token'); else f.set('upload_token', token); return f; };
  for (const token of [undefined, claim_secret, 'ab'.repeat(32)]) {
    assert.equal((await call(env, 'POST', '/api/kyc/upload', { form: form(token) })).status, 403, `upload with ${token}`);
    assert.equal((await call(env, 'POST', '/api/kyc/submit', { body: { session_id: session, upload_token: token } })).status, 403, `submit with ${token}`);
    assert.equal((await call(env, 'POST', '/api/kyc/handoff', { body: { session_id: session, upload_token: token } })).status, 403, `handoff with ${token}`);
  }
  assert.equal(env.KYC_BUCKET.objects.size, 0);
  assert.equal((await call(env, 'POST', '/api/kyc/upload', { form: form(upload_token) })).status, 200);
});

test('the secret is in no log, in no response after the first, and stored nowhere', async () => {
  const env = environment();
  const logged = [];
  const saved = {};
  for (const k of ['log', 'error', 'warn', 'info', 'debug']) { saved[k] = console[k]; console[k] = (...a) => { logged.push(a.map((x) => (typeof x === 'string' ? x : JSON.stringify(x, Object.getOwnPropertyNames(x || {})))).join(' ')); }; }
  const later = [];
  let secret;
  try {
    const start = await call(env, 'POST', '/api/kyc/start');
    secret = start.body.claim_secret;
    const { session_id, upload_token } = start.body;
    uploadTokens.set(session_id, upload_token);
    for (const side of ['front', 'back']) later.push(await call(env, 'POST', '/api/kyc/upload', { form: uploadForm(session_id, side), headers: { 'cf-ipcountry': 'US' } }));
    later.push(await call(env, 'POST', '/api/kyc/handoff', { body: { session_id, upload_token, full_name: 'Ada Lovelace', birthday: '1990-01-01' } }));
    later.push(await call(env, 'GET', `/api/kyc/handoff/${session_id}`));
    await withXai(() => xaiText(APPROVE), async () => { later.push(await submit(env, session_id)); });
    later.push(await call(env, 'GET', `/api/kyc/session/${session_id}`));
    later.push(await call(env, 'GET', `/api/kyc/status/kyc-${session_id}-front`));
    const reg = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: session_id, kyc_claim_secret: secret }) });
    assert.equal(reg.status, 200, JSON.stringify(reg.body));
    later.push(reg);
    later.push(await call(env, 'POST', '/api/auth/register', { body: registerBody(2, { kyc_session_id: session_id, kyc_claim_secret: secret }) }));
    later.push(await call(env, 'GET', '/health'));
  } finally {
    Object.assign(console, saved);
  }
  assert.match(secret, /^[0-9a-f]{64}$/);
  for (const r of later) assert.ok(!JSON.stringify(r.body).includes(secret), `a response carried the secret: ${JSON.stringify(r.body).slice(0, 120)}`);
  for (const line of logged) assert.ok(!line.includes(secret), `a log line carried the secret: ${line.slice(0, 120)}`);
  for (const ns of [env.KYC_STATUS, env.SCREENING, env.USERS, env.SOCIAL, env.AUDIT_LOG])
    for (const [k, v] of ns.store) assert.ok(!k.includes(secret) && !String(v).includes(secret), `stored under ${k}`);
  for (const [k, o] of env.KYC_BUCKET.objects) assert.ok(!k.includes(secret) && !Buffer.from(o.body).toString('latin1').includes(secret), `stored in ${k}`);
});

test('the hand-off token (the phone\'s) can upload and submit, and can neither register nor link', async () => {
  const env = environment();
  const start = (await call(env, 'POST', '/api/kyc/start')).body;
  const handoff = await call(env, 'POST', '/api/kyc/handoff', { body: { session_id: start.session_id, upload_token: start.upload_token, full_name: 'Ada Lovelace', birthday: '1990-01-01' } });
  const link = new URL(handoff.body.verify_url);
  assert.equal(link.searchParams.get('s'), start.session_id);
  assert.equal(link.hash, `#t=${start.upload_token}`);
  assert.ok(!handoff.body.verify_url.includes(start.claim_secret));
  // The phone, with what the link gave it.
  const phoneToken = link.hash.slice('#t='.length);
  uploadTokens.set(start.session_id, phoneToken);
  await uploadBoth(env, start.session_id);
  await withXai(() => xaiText(APPROVE), async () => assert.equal((await submit(env, start.session_id)).body.status, 'verified'));
  const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: start.session_id, kyc_claim_secret: phoneToken }) });
  assert.equal(r.status, 403);
  const OLD = '13131313-1313-4313-8313-131313131313';
  await env.USERS.put(`user:${OLD}`, JSON.stringify({ id: OLD, username: 'old_phone' }));
  assert.equal((await call(env, 'POST', '/api/kyc/link', { token: mintToken(OLD), body: { kyc_session_id: start.session_id, kyc_claim_secret: phoneToken } })).status, 403);
  // The desktop, with the claim secret.
  assert.equal((await call(env, 'POST', '/api/auth/register', { body: registerBody(2, { kyc_session_id: start.session_id, kyc_claim_secret: start.claim_secret }) })).status, 200);
});

test('id_hash is an HMAC under KYC_ID_PEPPER; without the pepper sign-up and link fail closed and write nothing', async () => {
  const env = environment();
  const session = await seedVerifiedKyc(env, { name: 'Ada King Lovelace', dob: '1815-12-10' });
  const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: session }) });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const user = JSON.parse(await env.USERS.get(`user:${r.body.user.id}`));
  assert.equal(user.id_hash, hmac(TEST_KYC_ID_PEPPER, 'eustress-kyc-person-v1|ada king lovelace|1815-12-10'));
  assert.notEqual(user.id_hash, sha256('eustress-kyc-person-v1|ada king lovelace|1815-12-10'), 'not a plain SHA-256');

  for (const KYC_ID_PEPPER of [undefined, '', 'short']) {
    const bare = environment({ KYC_ID_PEPPER });
    const s = await seedVerifiedKyc(bare);
    const before = [...bare.USERS.store.keys()].length;
    const refused = await call(bare, 'POST', '/api/auth/register', { body: registerBody(3, { kyc_session_id: s }) });
    assert.equal(refused.status, 503, JSON.stringify(refused.body));
    assert.equal(refused.body.code, 'kyc_unconfigured');
    assert.match(refused.body.error, /KYC_ID_PEPPER/);
    assert.equal([...bare.USERS.store.keys()].length, before);
    assert.equal(bare.KYC_BUCKET.objects.size, 0);
    const OLD = '14141414-1414-4414-8414-141414141414';
    await bare.USERS.put(`user:${OLD}`, JSON.stringify({ id: OLD, username: 'old_pep' }));
    assert.equal((await call(bare, 'POST', '/api/kyc/link', { token: mintToken(OLD), body: { kyc_session_id: s, kyc_claim_secret: claimSecretOf(s) } })).status, 503);
    assert.equal((await call(bare, 'GET', '/health')).body.integrations.kyc_id_pepper.configured, false);
  }
  assert.equal((await call(env, 'GET', '/health')).body.integrations.kyc_id_pepper.configured, true);
  assert.ok(!JSON.stringify((await call(env, 'GET', '/health')).body).includes(TEST_KYC_ID_PEPPER));
});

test('two different sessions for one person, in parallel, make one account', async () => {
  const env = environment();
  const a = await seedVerifiedKyc(env, { name: 'Mary Somerville', dob: '1980-12-26' });
  const b = await seedVerifiedKyc(env, { name: 'SOMERVILLE Mary', dob: '1980-12-26' });
  // Both reach the claim before either writes it.
  const realPut = env.KYC_BUCKET.put.bind(env.KYC_BUCKET);
  const held = [];
  env.KYC_BUCKET.put = async (key, ...rest) => {
    if (key.startsWith('claims/idhash/') && held.length < 2) {
      const go = new Promise((resolve) => held.push(resolve));
      if (held.length === 2) held.forEach((resolve) => resolve());
      await go;
    }
    return realPut(key, ...rest);
  };
  const results = await Promise.all([a, b].map((s, i) => call(env, 'POST', '/api/auth/register', { body: registerBody(i, { kyc_session_id: s }) })));
  env.KYC_BUCKET.put = realPut;
  assert.equal(held.length, 2, 'both reached the claim');
  assert.deepEqual(results.map((r) => r.status).sort(), [200, 409], JSON.stringify(results.map((r) => r.body)));
  const lost = results.find((r) => r.status === 409).body;
  assert.equal(lost.code, 'kyc_id_in_use');
  assert.match(lost.error, /support@eustress\.dev/);
  assert.equal(userKeys(env).length, 1);
  // The loser's session was not consumed: a person's review can still let it through.
  const loser = results[0].status === 409 ? a : b;
  assert.equal(env.KYC_BUCKET.objects.has(`claims/kyc-session/${loser}`), false);
});

test('an old client-supplied idhash entry blocks nothing: uniqueness reads only the R2 claims', async () => {
  const env = environment();
  const session = await seedVerifiedKyc(env, { name: 'Emmy Noether', dob: '1982-03-23' });
  const expected = hmac(TEST_KYC_ID_PEPPER, 'eustress-kyc-person-v1|emmy noether|1982-03-23');
  // An old-style index entry, even one that happens to hold this very value, naming another account.
  await env.USERS.put(`idhash:${expected}`, '15151515-1515-4515-8515-151515151515');
  await env.USERS.put(`idhash:${'ab'.repeat(32)}`, '15151515-1515-4515-8515-151515151515');
  const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: session }) });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(JSON.parse(await env.USERS.get(`user:${r.body.user.id}`)).id_hash, expected);
});

test('a session that cannot be consumed gives the person back: the id_hash claim is released', async () => {
  const env = environment();
  const session = await seedVerifiedKyc(env, { name: 'Lise Meitner', dob: '1978-11-07' });
  await env.KYC_BUCKET.put(`claims/kyc-session/${session}`, '{"user_id":"someone-else"}');
  const r = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: session }) });
  assert.equal(r.status, 409, JSON.stringify(r.body));
  assert.equal(r.body.code, 'kyc_session_used');
  assert.equal([...env.KYC_BUCKET.objects.keys()].filter((k) => k.startsWith('claims/idhash/')).length, 0, 'released');
  assert.equal(userKeys(env).length, 0);
});

test('deleting an account deletes its claim on the person, so the person can register again', async () => {
  const env = environment();
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'root', kyc_verified: true }));
  const first = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: await seedVerifiedKyc(env, { name: 'Chien-Shiung Wu', dob: '1985-05-31' }) }) });
  assert.equal(first.status, 200);
  const blocked = await call(env, 'POST', '/api/auth/register', { body: registerBody(2, { kyc_session_id: await seedVerifiedKyc(env, { name: 'Chien-Shiung Wu', dob: '1985-05-31' }) }) });
  assert.equal(blocked.status, 409);
  assert.equal((await call(env, 'POST', '/api/admin/delete-user', { token: mintToken(ADMIN), body: { username: first.body.user.username } })).status, 200);
  assert.equal([...env.KYC_BUCKET.objects.keys()].filter((k) => k.startsWith('claims/idhash/')).length, 0, 'the claim went with the account');
  const again = await call(env, 'POST', '/api/auth/register', { body: registerBody(3, { kyc_session_id: await seedVerifiedKyc(env, { name: 'Chien-Shiung Wu', dob: '1985-05-31' }) }) });
  assert.equal(again.status, 200, JSON.stringify(again.body));
});

test('a true collision is resolved by an administrator\'s review, and by nothing else', async () => {
  const env = environment();
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'root', kyc_verified: true }));
  const holder = await call(env, 'POST', '/api/auth/register', { body: registerBody(1, { kyc_session_id: await seedVerifiedKyc(env, { name: 'John Smith', dob: '1990-01-01' }) }) });
  assert.equal(holder.status, 200);
  const other = await seedVerifiedKyc(env, { name: 'John Smith', dob: '1990-01-01' });
  assert.equal((await call(env, 'POST', '/api/auth/register', { body: registerBody(2, { kyc_session_id: other }) })).status, 409);

  const look = await call(env, 'GET', `/api/admin/kyc-collision?session=${other}`, { token: mintToken(ADMIN) });
  assert.equal(look.status, 200, JSON.stringify(look.body));
  assert.equal(look.body.holder, holder.body.user.id);
  assert.equal((await call(env, 'POST', '/api/admin/kyc-collision', { token: mintToken(holder.body.user.id), body: { kyc_session_id: other, reason: 'x' } })).status, 403, 'not an admin');
  assert.equal((await call(env, 'POST', '/api/admin/kyc-collision', { token: mintToken(ADMIN), body: { kyc_session_id: other } })).status, 400, 'a reason is required');
  const reviewed = await call(env, 'POST', '/api/admin/kyc-collision', { token: mintToken(ADMIN), body: { kyc_session_id: other, reason: 'Different people: different document numbers and photos, checked by hand.' } });
  assert.equal(reviewed.status, 200, JSON.stringify(reviewed.body));
  assert.ok([...env.AUDIT_LOG.store.values()].some((v) => JSON.parse(v).action === 'KYC_COLLISION_REVIEWED'));

  const second = await call(env, 'POST', '/api/auth/register', { body: registerBody(3, { kyc_session_id: other }) });
  assert.equal(second.status, 200, JSON.stringify(second.body));
  const a = JSON.parse(await env.USERS.get(`user:${holder.body.user.id}`));
  const b = JSON.parse(await env.USERS.get(`user:${second.body.user.id}`));
  assert.notEqual(a.id_hash, b.id_hash, 'each has an id_hash of their own');
  assert.equal(a.kyc_verified, true, 'the holder is untouched');
});

test('the session id alone reads the status and nothing about the applicant; the token in a header reads the rest, never the document name', async () => {
  const env = environment();
  const { session, claim_secret, upload_token } = await startKycSession(env);
  await uploadBoth(env, session);
  await withXai(() => xaiText(APPROVE), async () => { assert.equal((await submit(env, session)).body.status, 'verified'); });
  await env.KYC_STATUS.put(`handoff:${session}`, JSON.stringify({ session_id: session, id_type: 'Passport', needs_back: false, iso2: 'US', minimum_age: 18, accepted_ids: ['Passport'], full_name: 'Ada Lovelace', birthday: '1990-01-01' }));
  const paths = { session: `/api/kyc/session/${session}`, status: `/api/kyc/status/kyc-${session}-front`, handoff: `/api/kyc/handoff/${session}` };
  const read = (p, token) => call(env, 'GET', p, token === undefined ? {} : { headers: { 'X-KYC-Token': token } });

  // The bare id, a wrong token, and a token in the query string (ignored): status only.
  for (const [label, get] of [
    ['bare', (p) => read(p)],
    ['wrong token', (p) => read(p, 'ab'.repeat(32))],
    ['query string', (p) => call(env, 'GET', `${p}?token=${upload_token}&t=${upload_token}&X-KYC-Token=${claim_secret}`)],
  ]) {
    const s = await get(paths.session);
    assert.deepEqual([s.status, s.body], [200, { session_id: session, status: 'verified' }], `${label} session`);
    const v = await get(paths.status);
    assert.deepEqual([v.status, v.body], [200, { status: 'verified' }], `${label} status`);
    const h = await get(paths.handoff);
    assert.deepEqual([h.status, h.body], [200, { session_id: session, status: 'active' }], `${label} handoff`);
  }

  // The upload token or the claim secret, in the header: the details, and never the name read off the document.
  for (const token of [upload_token, claim_secret]) {
    const s = await read(paths.session, token);
    assert.equal(s.body.front_uploaded, true);
    assert.equal(s.body.age_verified, true);
    const v = await read(paths.status, token);
    assert.equal(v.body.age_verified, true);
    const h = await read(paths.handoff, token);
    assert.equal(h.body.needs_back, false);
    for (const r of [s, v, h]) {
      const text = JSON.stringify(r.body);
      for (const applicant of ['ocr_name', 'Ada', 'Lovelace', '1990-01-01']) assert.ok(!text.includes(applicant), `${applicant} in ${text}`);
    }
  }

  // An id nobody started is 404 on every route, token or not.
  const nobody = crypto.randomUUID();
  for (const p of [`/api/kyc/session/${nobody}`, `/api/kyc/status/kyc-${nobody}-front`, `/api/kyc/handoff/${nobody}`]) {
    assert.equal((await read(p)).status, 404, p);
    assert.equal((await read(p, upload_token)).status, 404, `${p} with another session's token`);
  }
});

test('no answer from submit carries the document holder\'s name or birth date, and a refusal that names them is not repeated', async () => {
  const holder = ['Ada', 'Lovelace', 'ADA', 'ada', '1990-01-01', '19900101', 'ocr_name', 'extracted'];
  const clean = (r, label) => { const text = JSON.stringify(r.body); for (const h of holder) assert.ok(!text.includes(h), `${label}: ${h} in ${text}`); };

  // Verified, and asked again (the stored verdict is returned).
  const env = environment();
  await uploadBoth(env);
  await withXai(() => xaiText(APPROVE), async () => {
    const first = await submit(env);
    assert.equal(first.body.status, 'verified');
    clean(first, 'verified');
    clean(await submit(env), 'verified again');
  });
  clean(await call(env, 'GET', `/api/kyc/session/${SESSION}`, { headers: { 'X-KYC-Token': tokenFor(SESSION) } }), 'session');

  // Refused, with a model sentence that names the holder or a date: a plain sentence instead.
  for (const reason of ['The name Ada Lovelace does not match the photo', 'Born 1990-01-01, the card is expired', 'Document number 12345678 is invalid']) {
    const e = environment();
    await uploadBoth(e);
    await withXai(() => xaiText(JSON.stringify({ doc_decision: 'DENY', doc_reason: reason, screening_decision: 'APPROVE', extracted_dob: '1990-01-01', extracted_name: 'Ada Lovelace' })), async () => {
      const r = await submit(e);
      assert.equal(r.body.status, 'rejected');
      assert.equal(r.body.reason, 'Document could not be verified', reason);
      clean(r, reason);
    });
    clean(await call(e, 'GET', `/api/kyc/status/kyc-${SESSION}-front`, { headers: { 'X-KYC-Token': tokenFor(SESSION) } }), `${reason}: status`);
  }
});

test('an account write that fails after the person is claimed releases the person and leaves no half account; the same person registers with a new verification', async () => {
  for (const failing of ['user:', 'username:', 'pubkey:', 'idhash:']) {
    const env = environment();
    const person = { name: 'Rosalind Franklin', dob: '1980-07-25' };
    const realPut = env.USERS.put.bind(env.USERS);
    let failed = 0;
    env.USERS.put = async (key, ...rest) => {
      if (key.startsWith(failing) && !failed) { failed++; throw new Error(`KV write failed for ${failing}`); }
      return realPut(key, ...rest);
    };
    const first = registerBody(1, { kyc_session_id: await seedVerifiedKyc(env, person) });
    const r = await call(env, 'POST', '/api/auth/register', { body: first });
    assert.equal(failed, 1, `${failing} was written and failed`);
    assert.equal(r.status, 503, `${failing}: ${JSON.stringify(r.body)}`);
    assert.equal(r.body.code, 'signup_failed');
    assert.equal([...env.KYC_BUCKET.objects.keys()].filter((k) => k.startsWith('claims/idhash/')).length, 0, `${failing}: the person was released`);
    assert.equal(userKeys(env).length, 0, `${failing}: no account record is left`);
    assert.equal(await env.USERS.get(`username:${first.username}`), null, `${failing}: no username index is left`);
    assert.equal(await env.USERS.get(`pubkey:${first.public_key}`), null, `${failing}: no key index is left, so nothing can sign in`);
    assert.equal([...env.USERS.store.keys()].filter((k) => k.startsWith('idhash:')).length, 0, `${failing}: no id_hash index is left`);
    // The consumed session stays consumed; a new verification of the same person registers.
    const reused = await call(env, 'POST', '/api/auth/register', { body: first });
    assert.deepEqual([reused.status, reused.body.code], [409, 'kyc_session_used'], `${failing}: the session stays consumed`);
    assert.equal([...env.KYC_BUCKET.objects.keys()].filter((k) => k.startsWith('claims/idhash/')).length, 0, `${failing}: and the person is released again`);
    const again = await call(env, 'POST', '/api/auth/register', { body: registerBody(2, { kyc_session_id: await seedVerifiedKyc(env, person) }) });
    assert.equal(again.status, 200, `${failing}: ${JSON.stringify(again.body)}`);
  }
});

test('a link whose account write fails releases the person it claimed', async () => {
  const env = environment();
  const OLD = '16161616-1616-4616-8616-161616161616';
  await env.USERS.put(`user:${OLD}`, JSON.stringify({ id: OLD, username: 'old_link' }));
  const realPut = env.USERS.put.bind(env.USERS);
  let failed = 0;
  env.USERS.put = async (key, ...rest) => {
    if (key === `user:${OLD}` && !failed) { failed++; throw new Error('KV write failed'); }
    return realPut(key, ...rest);
  };
  const kyc_session_id = await seedVerifiedKyc(env, { name: 'Dorothy Hodgkin', dob: '1982-05-12' });
  const r = await call(env, 'POST', '/api/kyc/link', { token: mintToken(OLD), body: { kyc_session_id, kyc_claim_secret: claimSecretOf(kyc_session_id) } });
  assert.equal(r.status, 503, JSON.stringify(r.body));
  assert.equal([...env.KYC_BUCKET.objects.keys()].filter((k) => k.startsWith('claims/idhash/')).length, 0, 'released');
  assert.equal(JSON.parse(await env.USERS.get(`user:${OLD}`)).kyc_verified, undefined);
  const again = await call(env, 'POST', '/api/kyc/link', { token: mintToken(OLD), body: (() => { return {}; })() });
  assert.equal(again.status, 403, 'nothing linked without a new verification');
  const fresh = await seedVerifiedKyc(env, { name: 'Dorothy Hodgkin', dob: '1982-05-12' });
  assert.equal((await call(env, 'POST', '/api/kyc/link', { token: mintToken(OLD), body: { kyc_session_id: fresh, kyc_claim_secret: claimSecretOf(fresh) } })).status, 200);
});
