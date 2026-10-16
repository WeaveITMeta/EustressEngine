// The Grok model through the real Worker: /health and the admin state route
// report the resolved model, and the hourly cron is what adopts a newer one.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, mintToken, ctx } from './helpers/load_worker.mjs';

const get = (path, token) => new Request(`https://api.eustress.dev${path}`, { headers: token ? { Authorization: `Bearer ${token}` } : {} });

test('/health and GET /api/admin/grok-model report the resolved model; the state route is for administrators', async () => {
  const worker = await loadWorker();
  const env = fakeEnv();
  assert.equal((await (await worker.fetch(get('/health'), env, ctx)).json()).model, 'grok-4.7');
  assert.equal((await worker.fetch(get('/api/admin/grok-model', mintToken('member-1')), env, ctx)).status, 403);
  await env.USERS.put('admin:admin-9', '1');
  const r = await worker.fetch(get('/api/admin/grok-model', mintToken('admin-9')), env, ctx);
  assert.equal(r.status, 200);
  const body = await r.json();
  assert.equal(body.model, 'grok-4.7');
  assert.equal(typeof body.price_known, 'boolean');
  assert.equal(body.auto_update, 'on');
});

test('the hourly cron adopts a newer flagship after its canary, and /health then reports it', async () => {
  const worker = await loadWorker();
  const sent = [];
  const env = { ...fakeEnv(), GROK_API_KEY: 'xai-test-key-not-real', EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: class { constructor(from, to, raw) { this.raw = raw; } } };
  const realFetch = globalThis.fetch;
  const asked = [];
  globalThis.fetch = async (url, init = {}) => {
    asked.push(String(url));
    if (url === 'https://api.x.ai/v1/language-models') return new Response(JSON.stringify({ models: [
      { id: 'grok-4.7', aliases: [], input_modalities: ['text', 'image'], output_modalities: ['text'], created: 1_790_000_000 },
      { id: 'grok-4.8', aliases: [], input_modalities: ['text', 'image'], output_modalities: ['text'], created: 1_795_000_000 },
    ] }), { status: 200 });
    if (url === 'https://api.x.ai/v1/responses' && JSON.parse(init.body).model === 'grok-4.8') return new Response(JSON.stringify({ output: [{ type: 'message', content: [{ type: 'output_text', text: '{"canary":"ok","sum":42}' }] }] }), { status: 200 });
    throw new Error('offline in tests');
  };
  try {
    const waits = [];
    await worker.scheduled({ scheduledTime: Date.parse('2026-10-08T05:00:00.000Z') }, env, { waitUntil: (p) => waits.push(p), passThroughOnException() {} });
    await Promise.all(waits);
  } finally {
    globalThis.fetch = realFetch;
  }
  assert.ok(asked.includes('https://api.x.ai/v1/language-models'), 'the cron fetched the list');
  assert.equal((await (await worker.fetch(get('/health'), env, ctx)).json()).model, 'grok-4.8');
  assert.ok(sent.some((m) => /Subject: Grok model switched to grok-4\.8/.test(m.raw)), 'the administrators were told');
});
