// The Grok model: one resolver, a default of grok-4.7, and an auto-update that
// treats xAI's model list as untrusted data. A newer flagship id is adopted
// only from xAI's own host, only when it is numerically newer, and only after
// a fixed canary call passes; a pin stops it; a failing new model is rolled
// back; every switch alerts the administrators; and the id is never prompt text.

import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {
  DEFAULT_GROK_MODEL, GROK_MODEL_KEY, XAI_MODELS_URL, XAI_RESPONSES_URL, LIMITS, CANARY_PROMPT,
  parseModelId, pickCandidate, variantAllowlist, resolveGrokModel, grokClient, readState,
  fetchModelList, runCanary, canaryBody, isCanaryAnswer, sweepGrokModel, checkRollback,
  handleGrokModelRoute, renderGrokAlert, isModelFailure, priceLine,
} from '../src/grok_model.mjs';
import { fakeEnv } from './helpers/load_worker.mjs';

const SRC = path.resolve(import.meta.dirname, '..', 'src');
const HOUR = 3600 * 1000;

// ── fakes ────────────────────────────────────────────────────────────────────

const jsonResponse = (obj, status = 200) => new Response(JSON.stringify(obj), { status, headers: { 'content-type': 'application/json' } });
const answer = (text) => jsonResponse({ output: [{ type: 'message', content: [{ type: 'output_text', text }] }] });
// A release time for a fixture id, in seconds: later digits are released later here, as they usually are. The test that pins
// grok-4.20 against grok-4.7 gives real release times instead, because there the digits and the dates disagree.
const releasedAt = (id) => { const m = /^grok-(\d{1,2})\.(\d{1,2})/.exec(String(id)); return m ? 1_700_000_000 + Number(m[1]) * 10_000_000 + Number(m[2]) * 100_000 : undefined; };
const listed = (id, extra = {}) => ({ id, aliases: [], input_modalities: ['text', 'image'], output_modalities: ['text'], created: releasedAt(id), ...extra });
// xAI's real list always carries the model in use; fixtures given as plain ids get the current default too.
const withCurrent = (models) => (models.every((m) => typeof m === 'string') ? [...models, ...['grok-4.6', 'grok-4.7'].filter((id) => !models.includes(id))] : models);
const createdOf = (ids) => Object.fromEntries(ids.filter((id) => releasedAt(id) !== undefined).map((id) => [id, releasedAt(id)]));
const GOOD = '{"canary":"ok","sum":42}';

/// xAI: the list at XAI_MODELS_URL, the canary (and any call) at XAI_RESPONSES_URL. Every call is recorded.
function xai({ models = [], canary = GOOD, canaryStatus = 200, listResponse = null, onCanary = null } = {}) {
  const calls = [];
  const fn = async (url, init = {}) => {
    calls.push({ url, init, body: init.body ? JSON.parse(init.body) : null });
    if (url === XAI_MODELS_URL) return listResponse ? listResponse() : jsonResponse({ models: withCurrent(models).map((m) => (typeof m === 'string' ? listed(m) : m)) });
    if (url === XAI_RESPONSES_URL) {
      if (onCanary) await onCanary();
      return canaryStatus === 200 ? answer(canary) : new Response('upstream refused', { status: canaryStatus });
    }
    throw new Error(`unexpected url ${url}`);
  };
  fn.calls = calls;
  return fn;
}

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

function environment(extra = {}) {
  const sent = [];
  const env = { ...fakeEnv(), GROK_API_KEY: 'xai-test-key-not-real', EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage, ...extra };
  return { env, sent };
}

function deps(fetchImpl) {
  const audits = [];
  return {
    audits,
    fetch: fetchImpl,
    auditLog: async (env, action, adminId, target, details) => { audits.push({ action, adminId, target, details }); },
    requireAdmin: async (request) => ((request.headers.get('Authorization') || '') === 'Bearer admin-1' ? 'admin-1' : null),
    json: (data, status, headers) => new Response(JSON.stringify(data), { status, headers: { ...headers, 'Content-Type': 'application/json' } }),
  };
}

const subjects = (sent) => sent.map((m) => /^Subject: (.*)$/m.exec(m.raw)?.[1]?.trim() || '');
/// The text/plain part of a sent message, decoded.
const plainText = (raw) => Buffer.from(/Content-Type: text\/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n([\s\S]*?)\r\n--/.exec(raw)[1].replace(/\s+/g, ''), 'base64').toString('utf8');
const putState = (env, s) => env.SOCIAL.put(GROK_MODEL_KEY, JSON.stringify(s));

// ── the default, and one resolver ───────────────────────────────────────────

test('the default model is grok-4.7, with no state and with no KV at all', async () => {
  assert.equal(DEFAULT_GROK_MODEL, 'grok-4.7');
  assert.equal(await resolveGrokModel(fakeEnv()), 'grok-4.7');
  assert.equal(await resolveGrokModel({}), 'grok-4.7');
});

test('one resolver: no source file names a Grok model of its own, and index.js reaches xAI only through the client', () => {
  // moderation.mjs is owned by the moderation work running alongside; its research constant is reported, not changed here.
  const owned = new Set(['grok_model.mjs', 'review_costs.mjs', 'moderation.mjs']);
  for (const file of fs.readdirSync(SRC).filter((f) => /\.(mjs|js)$/.test(f) && !owned.has(f))) {
    const text = fs.readFileSync(path.join(SRC, file), 'utf8');
    assert.doesNotMatch(text, /['"`]grok-\d/, `${file} holds a Grok model string`);
    assert.doesNotMatch(text, /\bGROK_MODEL\b/, `${file} still has a GROK_MODEL constant`);
    assert.doesNotMatch(text, /['"`]https:\/\/api\.x\.ai/, `${file} calls xAI without the client`);
  }
  // review_costs.mjs names models only as price keys.
  for (const line of fs.readFileSync(path.join(SRC, 'review_costs.mjs'), 'utf8').split('\n').filter((l) => /['"`]grok-\d/.test(l))) {
    assert.match(line, /^\s*'grok-\d+\.\d+': Object\.freeze\(/, `review_costs.mjs uses a Grok model outside the price table: ${line.trim()}`);
  }
  const index = fs.readFileSync(path.join(SRC, 'index.js'), 'utf8');
  const clients = index.match(/await grokFor\(env\)/g) || [];
  assert.ok(clients.length >= 3, `KYC, screening and moderation each take a client (found ${clients.length})`);
  // The model a call site holds is only ever recorded as a `model` value, never put in a prompt.
  for (const line of index.split('\n').filter((l) => /\bgrok\.model\b/.test(l) && !/^\s*\/\//.test(l))) {
    assert.match(line, /^\s*(model|grokModel): grok\.model,\s*$/, `grok.model used outside a model field: ${line.trim()}`);
  }
});

test('the client sends the resolved model, and a caller cannot override it or store: false', async () => {
  const { env } = environment();
  await putState(env, { current: 'grok-4.9', previous: 'grok-4.7', pinned: null, history: [], adopted_by: 'admin' });
  const net = xai();
  const grok = await grokClient(env, deps(net));
  assert.equal(grok.model, 'grok-4.9');
  await grok.fetch({ input: 'hello', model: 'grok-1.0', store: true }, 'k');
  assert.equal(net.calls[0].url, XAI_RESPONSES_URL);
  assert.equal(net.calls[0].body.model, 'grok-4.9');
  assert.equal(net.calls[0].body.store, false);
});

test('a stored model that fails the pattern is not sent: the default is', async () => {
  const { env } = environment();
  await putState(env, { current: 'grok-4.9"; DROP', pinned: '../../etc', history: [] });
  assert.equal(await resolveGrokModel(env), DEFAULT_GROK_MODEL);
});

// ── the pattern ─────────────────────────────────────────────────────────────

test('ids outside the flagship pattern are rejected', () => {
  const none = new Set();
  for (const bad of ['grok-4.8; DROP', 'grok-4.8\n', 'grok-4.8 ', ' grok-4.8', '../', '../grok-4.8', 'grok-4.8/../x', 'grok-' + '9'.repeat(5000) + '.1',
    'grok-4.8' + 'x'.repeat(100), 'GROK-4.8', 'grok-4', 'grok-4.8.1', 'grok-100.1', 'grok-4.100', 'grok-4.8-fast', 'grok-4.8-mini', 'grok-4.8-0709',
    'grok-4.8-latest', 'latest', '', null, undefined, 4.8, { toString: () => 'grok-4.8' }, ['grok-4.8']]) {
    assert.equal(parseModelId(bad, none), null, `accepted ${JSON.stringify(String(bad)).slice(0, 40)}`);
  }
  assert.deepEqual(parseModelId('grok-4.8', none), { major: 4, minor: 8, variant: null });
  assert.deepEqual(parseModelId('grok-10.12', none), { major: 10, minor: 12, variant: null });
});

test('a variant is accepted only when GROK_MODEL_VARIANTS names its suffix', () => {
  assert.equal(parseModelId('grok-4.8-fast', variantAllowlist({})), null);
  assert.deepEqual(parseModelId('grok-4.8-fast', variantAllowlist({ GROK_MODEL_VARIANTS: 'fast, mini' })), { major: 4, minor: 8, variant: 'fast' });
  assert.equal(parseModelId('grok-4.8-0709', variantAllowlist({ GROK_MODEL_VARIANTS: 'fast' })), null);
  // A suffix that is not letters and digits is never on the list.
  assert.deepEqual([...variantAllowlist({ GROK_MODEL_VARIANTS: 'fast,../x,a b,MINI' })], ['fast', 'mini']);
});

test('the candidate is the newest model xAI released after the current one', () => {
  const none = new Set();
  const pick = (ids, current, allow = none, blocked = {}, now = Date.now()) => pickCandidate(ids, current, allow, blocked, now, createdOf([...ids, current]));
  assert.equal(pick(['grok-4.7', 'grok-4.6', 'grok-3.9'], 'grok-4.7'), null, 'not newer');
  assert.equal(pick(['grok-4.10', 'grok-4.9'], 'grok-4.7'), 'grok-4.10', 'the latest release');
  assert.equal(pick(['grok-5.0', 'grok-4.12'], 'grok-4.7'), 'grok-5.0');
  assert.equal(pick(['grok-4.8-fast'], 'grok-4.7'), null, 'a variant not on the list');
  assert.equal(pick(['grok-4.8-fast', 'grok-4.8'], 'grok-4.7', new Set(['fast'])), 'grok-4.8', 'the flagship over its variant');
  assert.equal(pick(['grok-4.8; DROP', 'grok-4.8\n', '../'], 'grok-4.7'), null);
  const now = Date.now();
  assert.equal(pick(['grok-4.8'], 'grok-4.7', none, { 'grok-4.8': { until: new Date(now + HOUR).toISOString() } }, now), null, 'blocked');
  assert.equal(pick(['grok-4.8'], 'grok-4.7', none, { 'grok-4.8': { until: new Date(now - HOUR).toISOString() } }, now), 'grok-4.8', 'block lapsed');
  // No release times, or none for the model in use: nothing is adopted.
  assert.equal(pickCandidate(['grok-4.8'], 'grok-4.7', none), null, 'no release times');
  assert.equal(pickCandidate(['grok-4.8'], 'grok-4.7', none, {}, now, { 'grok-4.8': 1_800_000_000 }), null, 'the current model is not on the list');
  assert.equal(pickCandidate(['grok-4.8'], 'grok-4.7', none, {}, now, { 'grok-4.7': 1_700_000_000 }), null, 'the candidate has no release time');
});

test('digits are not age: grok-4.20 (March 2026) is older than grok-4.7 (September 2026)', () => {
  const none = new Set();
  // Release times as xAI lists them: Grok 4.20 on 2026-03-09, Grok 4.7 on 2026-09-21.
  const created = { 'grok-4.20': Date.parse('2026-03-09T00:00:00Z') / 1000, 'grok-4.7': Date.parse('2026-09-21T00:00:00Z') / 1000 };
  assert.equal(pickCandidate(['grok-4.20', 'grok-4.7'], 'grok-4.7', none, {}, Date.now(), created), null, 'never downgrades to 4.20');
  assert.equal(pickCandidate(['grok-4.20', 'grok-4.7'], 'grok-4.20', none, {}, Date.now(), created), 'grok-4.7', 'a Worker left on 4.20 moves to 4.7');
});

// ── the list ────────────────────────────────────────────────────────────────

test('the list is fetched from the fixed xAI URL, a redirect is not followed, and another host is refused', async () => {
  const toEvil = xai({ listResponse: () => new Response(null, { status: 302, headers: { location: 'https://evil.example/v1/language-models' } }) });
  assert.deepEqual(await fetchModelList('k', { fetch: toEvil }), { ok: false, error: 'list_redirected' });
  assert.equal(toEvil.calls.length, 1, 'the redirect was not followed');
  assert.equal(toEvil.calls[0].url, 'https://api.x.ai/v1/language-models');
  assert.equal(toEvil.calls[0].init.redirect, 'manual');
  assert.ok(toEvil.calls[0].init.signal, 'the time is bounded');

  for (const where of ['https://evil.example/v1/language-models', 'https://api.x.ai.evil.example/v1/language-models', 'http://api.x.ai/v1/language-models']) {
    const elsewhere = xai({ listResponse: () => { const r = jsonResponse({ models: [listed('grok-4.8')] }); Object.defineProperty(r, 'url', { value: where }); return r; } });
    assert.deepEqual(await fetchModelList('k', { fetch: elsewhere }), { ok: false, error: 'list_wrong_host' }, where);
  }
  const followed = xai({ listResponse: () => { const r = jsonResponse({ models: [listed('grok-4.8')] }); Object.defineProperty(r, 'redirected', { value: true }); return r; } });
  assert.deepEqual(await fetchModelList('k', { fetch: followed }), { ok: false, error: 'list_wrong_host' });
});

test('an id from a redirected host is never adopted', async () => {
  const { env, sent } = environment();
  const net = xai({ listResponse: () => { const r = jsonResponse({ models: [listed('grok-4.9')] }); Object.defineProperty(r, 'url', { value: 'https://evil.example/models' }); return r; } });
  const out = await sweepGrokModel(env, deps(net));
  assert.equal(out.error, 'list_wrong_host');
  assert.equal(net.calls.filter((c) => c.url === XAI_RESPONSES_URL).length, 0, 'no canary');
  assert.equal(await resolveGrokModel(env), DEFAULT_GROK_MODEL);
  assert.equal(sent.length, 0);
});

test('the list is capped in size, needs the documented shape, and a text-only model is skipped', async () => {
  const big = xai({ listResponse: () => jsonResponse({ models: [listed('grok-4.8', { pad: 'x'.repeat(LIMITS.list_max_bytes) })] }) });
  assert.deepEqual(await fetchModelList('k', { fetch: big }), { ok: false, error: 'list_too_large' });
  const declared = xai({ listResponse: () => new Response('{}', { status: 200, headers: { 'content-length': String(LIMITS.list_max_bytes + 1) } }) });
  assert.deepEqual(await fetchModelList('k', { fetch: declared }), { ok: false, error: 'list_too_large' });
  const bare = xai({ listResponse: () => jsonResponse([listed('grok-4.8')]) });
  assert.deepEqual(await fetchModelList('k', { fetch: bare }), { ok: false, error: 'list_unexpected_shape' });
  const textOnly = xai({ models: [listed('grok-4.9', { input_modalities: ['text'] }), listed('grok-4.8', { aliases: ['grok-4.8-latest'] })] });
  const textOnlyList = await fetchModelList('k', { fetch: textOnly });
  assert.deepEqual({ ...textOnlyList, created: { ...textOnlyList.created } }, { ok: true, ids: ['grok-4.8', 'grok-4.8-latest'], created: { 'grok-4.8': releasedAt('grok-4.8'), 'grok-4.8-latest': releasedAt('grok-4.8') } }, 'an alias carries its model\'s release time');
});

// ── the canary ──────────────────────────────────────────────────────────────

test('the canary answer has one fixed shape', () => {
  assert.equal(isCanaryAnswer(GOOD), true);
  assert.equal(isCanaryAnswer('```json\n{"canary": "ok", "sum": 42}\n```'), true);
  for (const bad of ['{"canary":"ok","sum":41}', '{"canary":"ok","sum":"42"}', '{"canary":"ok","sum":42,"x":1}', 'ok', '', '[42]', 'Sure! {"canary":"ok","sum":42}']) {
    assert.equal(isCanaryAnswer(bad), false, bad);
  }
});

test('a newer valid id is adopted only after a passing canary, and the admins are alerted', async () => {
  const { env, sent } = environment();
  let stateAtCanary = null;
  const net = xai({ models: ['grok-4.6', 'grok-4.7', 'grok-4.8', 'grok-4.8; DROP', 'grok-9.9\n'], onCanary: async () => { stateAtCanary = await readState(env); } });
  const d = deps(net);
  const out = await sweepGrokModel(env, d);
  assert.deepEqual(out.switched, { from: 'grok-4.7', to: 'grok-4.8' });
  assert.equal(stateAtCanary.current, null, 'nothing had switched while the canary ran');
  const canaries = net.calls.filter((c) => c.url === XAI_RESPONSES_URL);
  assert.equal(canaries.length, 1);
  assert.equal(canaries[0].body.model, 'grok-4.8');
  const s = await readState(env);
  assert.equal(s.current, 'grok-4.8');
  assert.equal(s.previous, 'grok-4.7');
  assert.equal(s.adopted_by, 'auto');
  assert.ok(Date.parse(s.probation_until) > Date.now());
  assert.equal(s.history.at(-1).event, 'switch');
  assert.equal(await resolveGrokModel(env), 'grok-4.8');
  // The alert: one email to the security mailbox, and the audit log.
  assert.deepEqual(subjects(sent), ['Grok model switched to grok-4.8']);
  assert.equal(sent[0].to, 'security@eustress.dev');
  assert.ok(plainText(sent[0].raw).includes(priceLine('grok-4.8')), 'the alert says the price is unknown');
  assert.equal(d.audits.at(-1).action, 'GROK_MODEL_SWITCH');
  // Run again: nothing newer, nothing sent.
  const again = await sweepGrokModel(env, deps(xai({ models: ['grok-4.8'] })));
  assert.equal(again.candidate, null);
  assert.equal(sent.length, 1);
});

test('a model with a known price gets no price warning in its alert', () => {
  const priced = renderGrokAlert('switch', { from: 'grok-4.7', to: 'grok-4.8' }, { MODERATION_PRICES: JSON.stringify({ 'grok-4.8': { input_per_mtok: 1, output_per_mtok: 2 } }) });
  assert.ok(!priced.text.includes('price unknown'));
  const unpriced = renderGrokAlert('switch', { from: 'grok-4.7', to: 'grok-4.8' }, {});
  assert.ok(unpriced.text.includes('price unknown for grok-4.8; quotes incomplete until set'));
});

test('a failing canary keeps the current model, blocks the candidate for a day, and alerts', async () => {
  for (const [name, opts] of [['wrong answer', { canary: '{"canary":"ok","sum":41}' }], ['server error', { canaryStatus: 500 }], ['not found', { canaryStatus: 404 }], ['prose', { canary: 'I am grok-4.8.' }]]) {
    const { env, sent } = environment();
    const out = await sweepGrokModel(env, deps(xai({ models: ['grok-4.8'], ...opts })));
    assert.ok(out.canary_failed, name);
    assert.equal(await resolveGrokModel(env), DEFAULT_GROK_MODEL, name);
    const s = await readState(env);
    assert.equal(s.current, null, name);
    assert.ok(Date.parse(s.blocked['grok-4.8'].until) > Date.now() + 23 * HOUR, name);
    assert.equal(s.history.at(-1).event, 'canary_failed', name);
    assert.deepEqual(subjects(sent), ['Grok model grok-4.8 failed its canary; grok-4.7 kept'], name);
    // Blocked: the next run does not call the canary again.
    const net = xai({ models: ['grok-4.8'] });
    await sweepGrokModel(env, deps(net));
    assert.equal(net.calls.filter((c) => c.url === XAI_RESPONSES_URL).length, 0, name);
  }
});

test('the canary is refused from another host', async () => {
  const net = async () => { const r = answer(GOOD); Object.defineProperty(r, 'url', { value: 'https://evil.example/v1/responses' }); return r; };
  assert.deepEqual(await runCanary('grok-4.8', 'k', { fetch: net }), { ok: false, reason: 'wrong_host' });
  assert.deepEqual(await runCanary('grok-4.8\n', 'k', { fetch: net }), { ok: false, reason: 'bad_id' });
});

// ── the id is never prompt text ─────────────────────────────────────────────

/// Every string in `value` with its path, except the request's own `model` field.
function stringsOutsideModel(value, at = '') {
  if (typeof value === 'string') return [[at, value]];
  if (!value || typeof value !== 'object') return [];
  return Object.entries(value).flatMap(([k, v]) => (at === '' && k === 'model' ? [] : stringsOutsideModel(v, `${at}/${k}`)));
}

test('the model id never appears in any prompt text sent to xAI', async () => {
  assert.doesNotMatch(CANARY_PROMPT, /grok/i);
  for (const id of ['grok-4.8', 'grok-12.34']) {
    const { env } = environment();
    const net = xai({ models: [id] });
    await sweepGrokModel(env, deps(net));
    const grok = await grokClient(env, deps(net));
    assert.equal(grok.model, id);
    await grok.fetch({ input: 'a caller prompt' }, 'k');
    const sentToXai = net.calls.filter((c) => c.url === XAI_RESPONSES_URL);
    assert.equal(sentToXai.length, 2, 'the canary and one call');
    for (const c of sentToXai) {
      assert.equal(c.body.model, id);
      for (const [where, text] of stringsOutsideModel(c.body)) assert.ok(!text.includes(id) && !/grok-\d/.test(text), `${id} is in ${where}`);
      assert.ok(!c.url.includes(id), 'never in the URL');
    }
  }
  assert.deepEqual(stringsOutsideModel(canaryBody('grok-4.8')).map(([, t]) => t).filter((t) => t.includes('grok-4.8')), []);
});

// ── pin ─────────────────────────────────────────────────────────────────────

const req = (method, p, { admin = true, body } = {}) => new Request(`https://api.eustress.dev${p}`, {
  method, headers: { ...(admin ? { Authorization: 'Bearer admin-1' } : { Authorization: 'Bearer member' }), 'Content-Type': 'application/json' },
  ...(body !== undefined ? { body: JSON.stringify(body) } : {}),
});
const route = (env, d, method, p, opts) => handleGrokModelRoute(req(method, p, opts), new URL(`https://api.eustress.dev${p}`), env, {}, d);

test('the admin routes are for administrators, and a pin is checked like any id and canaried', async () => {
  const { env } = environment();
  const d = deps(xai({ canary: 'nope' }));
  assert.equal((await route(env, d, 'GET', '/api/admin/grok-model', { admin: false })).status, 403);
  assert.equal((await route(env, d, 'POST', '/api/admin/grok-model/pin', { admin: false, body: { model: 'grok-4.6' } })).status, 403);
  const state = await (await route(env, d, 'GET', '/api/admin/grok-model')).json();
  assert.equal(state.model, 'grok-4.7');
  assert.equal(state.auto_update, 'on');
  for (const bad of ['grok-4.8; DROP', 'grok-4.8\n', '../', 'grok-4.8-fast']) {
    const r = await route(env, d, 'POST', '/api/admin/grok-model/pin', { body: { model: bad } });
    assert.equal(r.status, 400, bad);
    assert.ok(!(await r.text()).includes(bad.trim()), 'the refusal does not echo the id');
  }
  const failed = await route(env, d, 'POST', '/api/admin/grok-model/pin', { body: { model: 'grok-4.6' } });
  assert.equal(failed.status, 422, 'a pin whose canary fails is refused');
  assert.equal(await resolveGrokModel(env), 'grok-4.7');
  assert.equal(await route(env, d, 'GET', '/api/other'), null, 'other paths are not handled');
});

test('a pin stops auto-update until it is unpinned, and both alert', async () => {
  const { env, sent } = environment();
  const d = deps(xai());
  const pinned = await route(env, d, 'POST', '/api/admin/grok-model/pin', { body: { model: 'grok-4.6' } });
  assert.equal(pinned.status, 200);
  assert.equal(await resolveGrokModel(env), 'grok-4.6');
  const net = xai({ models: ['grok-4.8', 'grok-5.0'] });
  assert.deepEqual(await sweepGrokModel(env, deps(net)), { skipped: 'pinned' });
  assert.equal(net.calls.length, 0, 'a pinned Worker does not even fetch the list');
  assert.equal(await resolveGrokModel(env), 'grok-4.6');
  assert.equal((await (await route(env, d, 'GET', '/api/admin/grok-model')).json()).auto_update, 'off: pinned');

  assert.equal((await route(env, d, 'POST', '/api/admin/grok-model/unpin')).status, 200);
  const out = await sweepGrokModel(env, deps(xai({ models: ['grok-4.8', 'grok-5.0'] })));
  assert.deepEqual(out.switched, { from: 'grok-4.6', to: 'grok-5.0' });
  assert.deepEqual(subjects(sent), ['Grok model pinned to grok-4.6', 'Grok model unpinned; auto-update is on', 'Grok model switched to grok-5.0']);
});

test('a pin made while the canary runs wins', async () => {
  const { env } = environment();
  const d = deps(xai());
  const net = xai({ models: ['grok-4.8'], onCanary: async () => { await route(env, deps(async () => answer(GOOD)), 'POST', '/api/admin/grok-model/pin', { body: { model: 'grok-4.6' } }); } });
  const out = await sweepGrokModel(env, { ...d, fetch: net });
  assert.deepEqual(out, { skipped: 'pinned' });
  assert.equal(await resolveGrokModel(env), 'grok-4.6');
});

test('GROK_MODEL_AUTO_UPDATE=off stops the sweep, and no key means no list fetch', async () => {
  const net = xai({ models: ['grok-4.8'] });
  assert.deepEqual(await sweepGrokModel(environment({ GROK_MODEL_AUTO_UPDATE: 'off' }).env, deps(net)), { skipped: 'auto_update_off' });
  assert.deepEqual(await sweepGrokModel(environment({ GROK_API_KEY: '' }).env, deps(net)), { skipped: 'no_key' });
  assert.equal(net.calls.length, 0);
});

// ── rollback ────────────────────────────────────────────────────────────────

async function onProbation(env, adoptedAt = Date.now() - HOUR) {
  await putState(env, {
    current: 'grok-4.8', previous: 'grok-4.7', pinned: null, history: [], adopted_by: 'auto',
    adopted_at: new Date(adoptedAt).toISOString(), probation_until: new Date(adoptedAt + LIMITS.probation_ms).toISOString(), blocked: {},
  });
}

/// `n` calls through the client, the first `failures` of them answered 500.
async function calls(env, n, failures, status = 500) {
  let i = 0;
  const net = async () => (i++ < failures ? new Response('down', { status }) : answer('fine'));
  const grok = await grokClient(env, deps(net));
  assert.equal(grok.model, 'grok-4.8');
  for (let k = 0; k < n; k++) await grok.fetch({ input: 'x' }, 'k');
}

test(`a new model failing ${LIMITS.rollback_failure_rate * 100}% or more of at least ${LIMITS.rollback_min_calls} calls is rolled back, blocked, and alerted`, async () => {
  const { env, sent } = environment();
  await onProbation(env);
  await calls(env, 10, 3);
  const d = deps(xai({ models: ['grok-4.8'] }));
  const out = await sweepGrokModel(env, d);
  assert.deepEqual(out.rolled_back, { rolled_back: true, from: 'grok-4.8', to: 'grok-4.7', calls: 10, failures: 3 });
  assert.equal(await resolveGrokModel(env), 'grok-4.7');
  const s = await readState(env);
  assert.equal(s.adopted_by, 'rollback');
  assert.ok(Date.parse(s.blocked['grok-4.8'].until) > Date.now() + 6 * 24 * HOUR);
  assert.deepEqual(subjects(sent), ['Grok model rolled back from grok-4.8 to grok-4.7']);
  assert.equal(d.audits.at(-1).action, 'GROK_MODEL_ROLLBACK');
  // The failed model is not adopted again while it is blocked.
  const net = xai({ models: ['grok-4.8'] });
  await sweepGrokModel(env, deps(net));
  assert.equal(net.calls.filter((c) => c.url === XAI_RESPONSES_URL).length, 0);
  assert.equal(await resolveGrokModel(env), 'grok-4.7');
});

test('below the threshold, or below the minimum calls, there is no rollback', async () => {
  for (const [n, failures] of [[10, 2], [LIMITS.rollback_min_calls - 1, LIMITS.rollback_min_calls - 1]]) {
    const { env, sent } = environment();
    await onProbation(env);
    await calls(env, n, failures);
    assert.equal((await checkRollback(env, deps(xai()))).rolled_back, false, `${failures} of ${n}`);
    assert.equal(await resolveGrokModel(env), 'grok-4.8');
    assert.equal(sent.length, 0);
  }
});

test('a rate limit or a bad key is not the model failing', async () => {
  assert.equal(isModelFailure(429), false);
  assert.equal(isModelFailure(401), false);
  for (const s of [400, 403, 404, 500, 503]) assert.equal(isModelFailure(s), true, String(s));
  const { env } = environment();
  await onProbation(env);
  await calls(env, 10, 10, 429);
  assert.equal((await checkRollback(env, deps(xai()))).rolled_back, false);
});

test('a model past its probation, pinned, or chosen by an admin is not rolled back', async () => {
  const { env } = environment();
  await onProbation(env, Date.now() - LIMITS.probation_ms - HOUR);
  assert.deepEqual(await checkRollback(env, deps(xai())), { rolled_back: false });
  await putState(env, { current: 'grok-4.8', previous: 'grok-4.7', pinned: 'grok-4.8', adopted_by: 'admin', history: [] });
  assert.deepEqual(await checkRollback(env, deps(xai())), { rolled_back: false });
});
