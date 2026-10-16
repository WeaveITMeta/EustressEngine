// KYC sessions for tests (src/kyc_identity.mjs).
//
// startKycSession: a session as POST /api/kyc/start leaves it (the record holds
// only the SHA-256 of its claim secret and upload token), with the secrets
// handed back to the test. seedVerifiedKyc: a started session whose document
// /api/kyc/submit has verified, for a test that registers or links an account.

import crypto from 'node:crypto';
import { fakeKv, fakeBucket } from './load_worker.mjs';

let made = 0;
const secrets = new Map();
const sha256 = (text) => crypto.createHash('sha256').update(text).digest('hex');

/// The Worker secret id_hash is keyed with, in tests.
export const TEST_KYC_ID_PEPPER = 'test-kyc-id-pepper-not-real-0123456789';

function bindings(env) {
  env.KYC_STATUS ||= fakeKv();
  env.KYC_BUCKET ||= fakeBucket();
  env.SCREENING ||= fakeKv();
  if (!('KYC_ID_PEPPER' in env)) env.KYC_ID_PEPPER = TEST_KYC_ID_PEPPER;
}

/// A started session: `{ session, claim_secret, upload_token }`.
export async function startKycSession(env, over = {}) {
  bindings(env);
  made += 1;
  const session = over.session || crypto.randomUUID();
  const claim_secret = crypto.randomBytes(32).toString('hex');
  const upload_token = crypto.randomBytes(32).toString('hex');
  await env.KYC_STATUS.put(`kycsess:${session}`, JSON.stringify({
    claim_hash: sha256(claim_secret), upload_hash: sha256(upload_token), created_at: new Date().toISOString(), ...(over.sessionRecord || {}),
  }));
  secrets.set(session, { claim_secret, upload_token });
  return { session, claim_secret, upload_token };
}

/// The claim secret and the upload token of a session a helper here started.
export const claimSecretOf = (session) => secrets.get(session)?.claim_secret;
export const uploadTokenOf = (session) => secrets.get(session)?.upload_token;

/// Starts a session, writes `kyc-<session>-front` as a verified record, and
/// returns the session id (its claim secret: claimSecretOf). Each call is a
/// different person (name), so two accounts never share an id_hash unless a
/// test asks for the same `name` and `dob`.
/// `over`: session, name, dob, verified_at, record (fields merged last).
export async function seedVerifiedKyc(env, over = {}) {
  const { session } = await startKycSession(env, over);
  const name = over.name ?? `Test Person ${made}`;
  const dob = over.dob ?? '1990-01-01';
  const now = new Date().toISOString();
  const record = {
    status: 'verified', upload_id: `session-${session}`, session_id: session, iso2: 'US', id_type: 'Passport', side: 'front',
    r2_key: `kyc-us-session-${session}/front.bin`, uploaded_at: now, hash: sha256(session),
    verified_at: over.verified_at ?? now, ocr_name: name, extracted_dob: dob, extracted_sex: '',
    age_status: 'verified', age_verified: true, document_age: 36, minimum_age: 18, jurisdiction: 'US',
    grok_analysis: {
      doc_decision: 'APPROVE', document_type: 'passport', extracted_name: name, extracted_dob: dob,
      screening_decision: 'APPROVE', screening_flags: [], risk_score: 0, model: 'grok-test',
    },
    ...(over.record || {}),
  };
  await env.KYC_STATUS.put(`kyc-${session}-front`, JSON.stringify(record));
  return session;
}
