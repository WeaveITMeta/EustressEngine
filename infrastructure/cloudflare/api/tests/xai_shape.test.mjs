// What the Worker sends to the xAI Responses API. Content (text, images) must
// sit inside a message item with `input_text` and `input_image` parts. Bare
// `{type:'text'}` or `{type:'image_url'}` items at the top level are refused by
// xAI with a 422, which failed every ID check and every moderation judgement
// closed. These tests drive the three callers through their real code and pin
// the shape of what each one sends.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeKv, fakeBucket, ctx } from './helpers/load_worker.mjs';
import { grokJudge, grokAgent } from '../src/moderation.mjs';
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

// ── The moderation judge ─────────────────────────────────────────────────────

test('the moderation judge sends one user message: policy text, the captures, then the case', async () => {
  const sent = [];
  const deps = { grokFetch: async (body) => { sent.push(body); return { ok: false, status: 500, text: async () => '' }; } };
  await grokJudge({
    sim: { name: 'Pong', description: '', genre: 'all', is_public: true }, dossier: {}, jev: {}, triage: {},
    captures: [{ contentType: 'image/png', base64: 'AAAA', label: 'north' }, { contentType: 'image/png', base64: 'BBBB', label: 'south' }],
    policyText: 'POLICY', policyHash: 'abc', env: { GROK_API_KEY: 'test' }, deps,
  });
  assert.equal(sent.length, 1);
  assertResponsesInput(sent[0].input);
  assert.equal(sent[0].input.length, 1, 'one user message');
  const kinds = sent[0].input[0].content.map((p) => p.type);
  assert.deepEqual(kinds, ['input_text', 'input_image', 'input_image', 'input_text'], 'policy first (cacheable), captures, then the case');
  assert.ok(sent[0].input[0].content.slice(1, 3).every((p) => p.detail === 'low'));
});

test('the moderation judge with no captures still sends a valid message', async () => {
  const sent = [];
  const deps = { grokFetch: async (body) => { sent.push(body); return { ok: false, status: 500, text: async () => '' }; } };
  await grokJudge({ sim: {}, dossier: {}, jev: {}, triage: {}, captures: [], policyText: 'P', policyHash: 'h', env: { GROK_API_KEY: 'test' }, deps });
  assertResponsesInput(sent[0].input);
  assert.deepEqual(sent[0].input[0].content.map((p) => p.type), ['input_text', 'input_text']);
});

// ── The moderation agent ─────────────────────────────────────────────────────

test('the moderation agent starts with a user message and echoes tool calls as function items', async () => {
  const sent = [];
  let round = 0;
  const deps = {
    extractGrokText: () => 'done',
    grokFetch: async (body) => {
      sent.push(structuredClone(body));
      round++;
      const output = round === 1 ? [{ type: 'function_call', call_id: 'call_1', name: 'moderation_get_case', arguments: '{}' }] : [];
      return { ok: true, status: 200, json: async () => ({ output }), text: async () => '' };
    },
  };
  const transcript = await grokAgent({
    caseRecord: { sim_id: 'x' }, playbookText: 'PLAYBOOK', env: { GROK_API_KEY: 'test' }, deps,
    execute: async () => ({ summary: 'ok' }),
  });
  assert.equal(transcript.error, null);
  assert.equal(sent.length, 2);
  for (const body of sent) assertResponsesInput(body.input);
  assert.deepEqual(sent[1].input.slice(1).map((i) => i.type), ['function_call', 'function_call_output']);
});

// ── ID verification, through the real upload and submit routes ───────────────

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);

test('ID verification sends the document images and the prompt as one user message', async () => {
  const worker = await loadWorker();
  const env = { ...fakeEnv(), GROK_API_KEY: 'xai-test-not-real', KYC_STATUS: fakeKv(), SCREENING: fakeKv(), KYC_BUCKET: fakeBucket() };
  const session = 'session-1234567890';

  for (const side of ['front', 'back']) {
    const form = new FormData();
    form.set('side', side);
    form.set('id_type', "Driver's license");
    form.set('session_id', session);
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
      body: JSON.stringify({ session_id: session, full_name: 'Test Person', birthday: '1990-01-01' }),
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
