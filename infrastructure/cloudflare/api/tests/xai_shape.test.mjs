// What the Worker sends to the xAI Responses API. Content (text, images) must
// sit inside a message item with `input_text` and `input_image` parts. Bare
// `{type:'text'}` or `{type:'image_url'}` items at the top level are refused by
// xAI with a 422, which failed every ID check and every moderation judgement
// closed. These tests drive the callers through their real code and pin the
// shape of what each one sends. Under the Guardian Code v2.1 the moderation
// judge is Claude (tests/moderation_code_v21.test.mjs pins its request) and the
// Grok agent is retired; the xAI caller in moderation is the researcher.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeKv, fakeBucket, ctx } from './helpers/load_worker.mjs';
import { startKycSession } from './helpers/kyc_session.mjs';
import { runResearcher, researcherRequest, researchFacts } from '../src/moderation.mjs';
import { grokClient, DEFAULT_GROK_MODEL } from '../src/grok_model.mjs';
import { imagePart, textPart, userMessage } from '../src/xai.mjs';

/// Throws unless `input` is a valid Responses API input list.
function assertResponsesInput(input) {
  assert.ok(Array.isArray(input) && input.length > 0, 'input is a non-empty list');
  for (const [i, item] of input.entries()) {
    const at = `input[${i}]`;
    if (item.type === 'function_call') {
      assert.ok(item.call_id && item.name, `${at} function_call has call_id and name`);
      continue;
    }
    if (item.type === 'function_call_output') {
      assert.ok(item.call_id && typeof item.output === 'string', `${at} function_call_output has call_id and string output`);
      continue;
    }
    assert.ok(['user', 'assistant', 'system', 'developer'].includes(item.role), `${at} is a message item with a role, not a bare ${item.type} part`);
    assert.ok(Array.isArray(item.content) && item.content.length > 0, `${at}.content is a list of parts`);
    for (const [j, part] of item.content.entries()) {
      const pat = `${at}.content[${j}]`;
      assert.ok(['input_text', 'input_image', 'output_text'].includes(part.type), `${pat} has type ${part.type}`);
      if (part.type === 'input_image') {
        assert.equal(typeof part.image_url, 'string', `${pat}.image_url is a string, not an object`);
        assert.ok(/^(data:image\/[a-z+.-]+;base64,|https:\/\/)/.test(part.image_url), `${pat}.image_url is a data or https URL`);
      } else {
        assert.equal(typeof part.text, 'string', `${pat}.text is a string`);
      }
    }
  }
}

test('the validator refuses the shape xAI refused', () => {
  assert.throws(() => assertResponsesInput([{ type: 'image_url', image_url: { url: 'data:image/png;base64,AAAA' } }, { type: 'text', text: 'x' }]));
  assert.throws(() => assertResponsesInput([{ type: 'text', text: 'x' }]));
  assert.throws(() => assertResponsesInput([{ role: 'user', content: [{ type: 'input_image', image_url: { url: 'data:image/png;base64,AAAA' } }] }]));
  assert.doesNotThrow(() => assertResponsesInput([userMessage([imagePart('data:image/png;base64,AAAA', 'low'), textPart('x')])]));
});

test('the helpers build the documented message', () => {
  assert.deepEqual(userMessage([imagePart('data:image/png;base64,AAAA', 'high'), textPart('read this')]), {
    role: 'user',
    content: [
      { type: 'input_image', image_url: 'data:image/png;base64,AAAA', detail: 'high' },
      { type: 'input_text', text: 'read this' },
    ],
  });
  assert.deepEqual(imagePart('https://example.com/a.png'), { type: 'input_image', image_url: 'https://example.com/a.png' });
});

// ── The moderation researcher ────────────────────────────────────────────────

const TWIN = { name: 'Port of Tacoma Terminal', description: 'Category: Digital twin\nObjective: Inspect how the Port of Tacoma stages containers.\nLesson: How a terminal moves cargo from ship to rail.\nReal-world link: A twin of the Port of Tacoma terminal from public aerial imagery.' };

test('the moderation researcher sends one user message, the instruction and the facts as text, with the web search tool', async () => {
  const sent = [];
  const deps = { grokModel: 'grok-test', grokFetch: async (body) => { sent.push(body); return { ok: false, status: 500, text: async () => '' }; } };
  const r = await runResearcher({ facts: researchFacts(TWIN), trigger: 'real_place_or_business', env: { GROK_API_KEY: 'test' }, deps });
  assert.equal(sent.length, 1);
  assertResponsesInput(sent[0].input);
  assert.equal(sent[0].input.length, 1, 'one user message');
  assert.deepEqual(sent[0].input[0].content.map((p) => p.type), ['input_text'], 'text only: the researcher never sees an image');
  assert.deepEqual(sent[0].tools, [{ type: 'web_search' }]);
  assert.deepEqual(sent[0], researcherRequest(researchFacts(TWIN)));
  assert.deepEqual([r.ran, r.error, r.findings], [true, 'researcher_unavailable_500', []], 'a failure is recorded, not thrown');
});

test('the researcher goes through the Grok client: the resolved model, store: false, and the web search tool reach xAI', async () => {
  const calls = [];
  const env = { SOCIAL: fakeKv() };
  const grok = await grokClient(env, { fetch: async (url, init) => { calls.push({ url: String(url), body: JSON.parse(init.body), signal: init.signal }); return new Response(JSON.stringify({ output: [] }), { status: 200 }); } });
  await runResearcher({ facts: researchFacts(TWIN), trigger: 'real_place_or_business', env: { GROK_API_KEY: 'xai-test-not-real' }, deps: { grokFetch: grok.fetch, grokModel: grok.model } });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, 'https://api.x.ai/v1/responses');
  assert.deepEqual([calls[0].body.model, calls[0].body.store], [DEFAULT_GROK_MODEL, false], 'the model src/grok_model.mjs resolves, and not stored');
  assert.deepEqual(calls[0].body.tools, [{ type: 'web_search' }]);
  assert.ok(calls[0].signal instanceof AbortSignal, 'bounded by the researcher\'s time limit');
  assertResponsesInput(calls[0].body.input);
});

// ── ID verification, through the real upload and submit routes ───────────────

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);

test('ID verification sends the document images and the prompt as one user message', async () => {
  const worker = await loadWorker();
  const env = { ...fakeEnv(), GROK_API_KEY: 'xai-test-not-real', KYC_STATUS: fakeKv(), SCREENING: fakeKv(), KYC_BUCKET: fakeBucket() };
  const { session, upload_token } = await startKycSession(env);

  for (const side of ['front', 'back']) {
    const form = new FormData();
    form.set('side', side);
    form.set('id_type', "Driver's license");
    form.set('session_id', session);
    form.set('upload_token', upload_token);
    form.set('document', new File([PNG], `${side}.png`, { type: 'image/png' }));
    const res = await worker.fetch(new Request('https://api.eustress.dev/api/kyc/upload', { method: 'POST', headers: { 'cf-ipcountry': 'US' }, body: form }), env, ctx);
    assert.equal(res.status, 200, `${side} upload: ${await res.clone().text()}`);
  }

  const calls = [];
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), body: JSON.parse(init.body), auth: init.headers.Authorization });
    return new Response(JSON.stringify({ output: [{ type: 'message', content: [{ type: 'output_text', text: '{"doc_decision":"APPROVE","screening_decision":"APPROVE","risk_score":0,"extracted_dob":"1990-01-01"}' }] }] }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try {
    const res = await worker.fetch(new Request('https://api.eustress.dev/api/kyc/submit', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ session_id: session, upload_token, full_name: 'Test Person', birthday: '1990-01-01' }),
    }), env, ctx);
    assert.ok(res.status < 500, `submit answered ${res.status}`);
  } finally {
    globalThis.fetch = realFetch;
  }

  const xai = calls.filter((c) => c.url === 'https://api.x.ai/v1/responses');
  assert.equal(xai.length, 1, 'one call to xAI');
  assert.equal(xai[0].auth, 'Bearer xai-test-not-real');
  assertResponsesInput(xai[0].body.input);
  assert.equal(xai[0].body.input.length, 1, 'one user message');
  assert.deepEqual(xai[0].body.input[0].content.map((p) => p.type), ['input_image', 'input_image', 'input_text'], 'front, back, then the prompt');
  assert.ok(xai[0].body.input[0].content.slice(0, 2).every((p) => p.detail === 'high'), 'the small print on an ID is read at high detail');
  assert.equal(xai[0].body.store, false, 'xAI is told not to store the request');
});
