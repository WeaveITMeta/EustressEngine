// What the ID upload accepts. The verification model reads JPEG and PNG only,
// so anything else is refused at upload with a message the applicant can act
// on, instead of passing upload and then waiting for a person.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeKv, fakeBucket, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();

const JPEG = new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 16, 0x4a, 0x46, 0x49, 0x46, 0, 1]);
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]);
const WEBP = new Uint8Array([0x52, 0x49, 0x46, 0x46, 0x24, 0, 0, 0, 0x57, 0x45, 0x42, 0x50]);
const PDF = new TextEncoder().encode('%PDF-1.4\n1 0 obj\n');

function environment() {
  return { ...fakeEnv(), KYC_STATUS: fakeKv(), SCREENING: fakeKv(), KYC_BUCKET: fakeBucket() };
}

async function upload(env, bytes, type, name = 'id') {
  const form = new FormData();
  form.set('side', 'front');
  form.set('id_type', 'Passport');
  form.set('session_id', 'session-1234567890');
  form.set('document', new File([bytes], name, { type }));
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/kyc/upload', { method: 'POST', headers: { 'cf-ipcountry': 'US' }, body: form }), env, ctx);
  return { status: res.status, body: await res.json() };
}

test('a JPEG and a PNG are accepted and stored', async () => {
  for (const [bytes, type] of [[JPEG, 'image/jpeg'], [PNG, 'image/png']]) {
    const env = environment();
    const res = await upload(env, bytes, type);
    assert.equal(res.status, 200, JSON.stringify(res.body));
    assert.equal(env.KYC_BUCKET.objects.size, 1);
  }
});

test('a WebP or a PDF is refused with a message that says what to upload, and nothing is stored', async () => {
  for (const [bytes, type] of [[WEBP, 'image/webp'], [PDF, 'application/pdf']]) {
    const env = environment();
    const res = await upload(env, bytes, type);
    assert.equal(res.status, 415);
    assert.equal(res.body.error, 'Please upload a JPEG or PNG photo of your ID.');
    assert.equal(env.KYC_BUCKET.objects.size, 0);
    assert.equal(env.KYC_STATUS.store.size, 0);
  }
});

test('a WebP or a PDF labelled as a JPEG gets the same message, not a format-mismatch error', async () => {
  for (const bytes of [WEBP, PDF]) {
    const env = environment();
    const res = await upload(env, bytes, 'image/jpeg');
    assert.equal(res.status, 415);
    assert.equal(res.body.error, 'Please upload a JPEG or PNG photo of your ID.');
    assert.equal(env.KYC_BUCKET.objects.size, 0);
  }
});

test('a file whose bytes are not an image at all is still refused as a mismatch', async () => {
  const env = environment();
  const res = await upload(env, new TextEncoder().encode('not an image, just text'), 'image/jpeg');
  assert.equal(res.status, 415);
  assert.equal(env.KYC_BUCKET.objects.size, 0);
});
