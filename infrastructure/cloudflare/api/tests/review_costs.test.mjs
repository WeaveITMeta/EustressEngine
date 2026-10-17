// Review cost metering (src/review_costs.mjs): what the publishing review spends on models, per MB, and the quote built on it.
// Measurement only: nothing is charged, and nothing here may change a moderation decision.

import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import {
  buildJevQuestions, runModerationCase, handleModerationRoute, loadCase, sweepModeration,
  JEV_MODEL, JEV_WORKERS_AI_MODEL, POLICY_HASH_ANCHORED,
} from '../src/moderation.mjs';
import {
  DEFAULT_REVIEW_PRICES, DEFAULT_QUOTE_CONFIG, reviewPricesFrom, quoteConfigFrom, normalizeUsage, callCost, callVector, zeroVector,
  createReviewMeter, meterKind, meterBytes, meterCall, persistReviewMeter, rollupReviewCosts, aggregateLines, mergeAggregates,
  reviewCostMetrics, reviewCostReport, quoteReview, costLedgerKey, costDayKey, BYTES_PER_MB, MAX_QUOTE_BYTES,
} from '../src/review_costs.mjs';
import { MAX_WORLD_BYTES } from '../src/world.mjs';
import { DEFAULT_GROK_MODEL } from '../src/grok_model.mjs';
import { GUARDIAN_POLICY_TEXT, MODERATION_PLAYBOOK_TEXT } from '../src/generated/policy_text.mjs';
import { loadWorker, fakeEnv, mintToken, ctx } from './helpers/load_worker.mjs';
import { fakeClaude, apiError, questionnaireAnswers } from './helpers/claude_fake.mjs';
import { questionnaireRecord } from '../src/rating_questionnaire.mjs';

// ── fakes ───────────────────────────────────────────────────────────────────

// KV with list metadata and cursors, as Workers KV has them.
function kv({ metadata = true } = {}) {
  const m = new Map();
  const meta = new Map();
  return {
    m, meta,
    get: async k => m.get(k) ?? null,
    put: async (k, v, opts = {}) => {
      m.set(k, typeof v === 'string' ? v : String(v));
      if (metadata && opts.metadata !== undefined) meta.set(k, structuredClone(opts.metadata)); else meta.delete(k);
    },
    delete: async k => { m.delete(k); meta.delete(k); },
    list: async ({ prefix = '', limit = 1000, cursor } = {}) => {
      const all = [...m.keys()].filter(k => k.startsWith(prefix)).sort();
      const start = cursor ? Number(cursor) : 0;
      const done = start + limit >= all.length;
      return { keys: all.slice(start, start + limit).map(name => ({ name, ...(meta.has(name) ? { metadata: structuredClone(meta.get(name)) } : {}) })), list_complete: done, ...(done ? {} : { cursor: String(start + limit) }) };
    },
  };
}

function r2() {
  const m = new Map();
  let versions = 0;
  const obj = (key, rec) => ({
    key, size: rec.bytes.byteLength, etag: rec.etag, httpEtag: `"${rec.etag}"`, httpMetadata: rec.httpMetadata || {}, customMetadata: rec.customMetadata || {}, checksums: rec.checksums || {}, version: rec.version,
    text: async () => new TextDecoder().decode(rec.bytes), arrayBuffer: async () => rec.bytes.slice(0), body: rec.bytes,
  });
  return {
    m,
    put: async (key, body, opts = {}) => {
      if (opts.onlyIf) {
        const cur = m.get(key);
        if (opts.onlyIf.etagDoesNotMatch === '*' && cur) return null;
        if (opts.onlyIf.etagMatches !== undefined && (!cur || cur.etag !== opts.onlyIf.etagMatches)) return null;
      }
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body).buffer : (body instanceof ArrayBuffer ? body : body.buffer);
      const rec = { version: String(++versions), bytes, etag: createHash('md5').update(Buffer.from(bytes)).digest('hex'), httpMetadata: opts.httpMetadata, customMetadata: opts.customMetadata, checksums: opts.sha256 ? { sha256: Uint8Array.from(Buffer.from(opts.sha256, 'hex')).buffer } : undefined };
      m.set(key, rec);
      return obj(key, rec);
    },
    get: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    head: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    delete: async key => { m.delete(key); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ objects: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(key => ({ key })) }),
  };
}

function environment(extra = {}) {
  return { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2(), JEV_API_KEY: 'jev-test', GROK_API_KEY: 'grok-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', ...extra };
}

const json = (value, status, headers) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });

// The same Jev answers, judge verdicts and listing as tests/moderation.test.mjs.
function jevAnswers(overrides = {}) {
  const q = buildJevQuestions();
  const answers = {};
  for (const [k, def] of Object.entries(q)) {
    if (def.type === 'noul') answers[k] = { type: 'noul', noul: k === 'metadata_consistency' || k === 'functional_purpose' || k === 'spatial_intent' || k === 'tag_fit' ? 0.9 : 0.02 };
    else if (def.type === 'choice') {
      const first = k === 'content_rating' ? 'all_ages' : k === 'content_kind' ? 'simulation' : Object.keys(def.criteria)[0];
      answers[k] = { type: 'choice', choice: first, probabilities: Object.fromEntries(Object.keys(def.criteria).map(c => [c, c === first ? 0.9 : 0.01])), confidence: 0.9 };
    } else if (def.type === 'score') {
      const levels = def.criteria;
      const pick = k === 'quality_band' ? 'solid_authored_experience' : levels[0];
      answers[k] = { type: 'score', score: levels.indexOf(pick) / (levels.length - 1), probabilities: Object.fromEntries(levels.map(l => [l, l === pick ? 0.85 : 0.03])), legend: Object.fromEntries(levels.map((l, i) => [l, i])), confidence: 0.85 };
    }
  }
  for (const [k, v] of Object.entries(overrides)) {
    if (typeof v === 'number') answers[k] = { type: 'noul', noul: v };
    else answers[k] = { ...answers[k], ...v };
  }
  return { model: 'jev-test', answers, usage: { input_tokens: 1234, output_tokens: 0 } };
}

function jevFetch(answers, calls = []) {
  return async (url, init) => {
    calls.push(JSON.parse(init.body));
    return new Response(JSON.stringify(typeof answers === 'function' ? answers(calls.length) : answers), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
}

const STATEMENT_TEXT = 'Category: Simulation\nObjective: Set the tide range and read the depth at the quay.\nLesson: How tidal range and basin shape set harbour depth.\nReal-world link: Models a tidal harbour from published tide tables.';

function verdict(v = {}) {
  const { scores = {}, ...rest } = v;
  const tests = Object.fromEntries(['build_quality', 'objective', 'lesson', 'real_world'].map(k => [k, { score: scores[k] ?? 2, evidence: `${k}: the tide gauge over the harbour basin in capture-1` }]));
  return { verdict: 'publish', articles: [], tests, statement_check: 'matches', rating_floor: 'all_ages', descriptors: [], is_game: false, is_fiction: false, featured_candidate: false, policy_version: '2.2', policy_hash: 'sha256:x', spatial_evidence: ['central plaza with four symmetric fountains', 'material variety across the arcade facades'], rationale: 'Deliberate composition, no prohibited category.', confidence: 0.9, flagged_for_review: false, ...rest };
}
const CITE_A4 = { id: 'A4', confidence: 0.9, evidence: 'Script Soul/lab/recipe.luau lists reagent quantities and steps' };

// What xAI's Responses API reports for a call: cached tokens inside input_tokens, reasoning inside output_tokens.
const XAI_USAGE = { input_tokens: 9000, input_tokens_details: { cached_tokens: 4500 }, output_tokens: 400, output_tokens_details: { reasoning_tokens: 0 }, total_tokens: 9400 };
// $ at grok-4.6's list price: 4,500 uncached at $2/M, 4,500 cached at $0.50/M, 400 output at $6/M.
const XAI_USAGE_USD = 4500 * 2 / 1e6 + 4500 * 0.5 / 1e6 + 400 * 6 / 1e6;

const response = (body, status = 200) => ({ ok: status >= 200 && status < 300, status, json: async () => body, text: async () => JSON.stringify(body) });

// What Anthropic's Messages API reports for a judge call: uncached input apart from cache reads and cache writes, thinking
// inside output_tokens.
const CLAUDE_USAGE = { input_tokens: 900, cache_creation_input_tokens: 300, cache_read_input_tokens: 8000, output_tokens: 1200 };
// $ at claude-haiku-5-5's price: 900 uncached at $0.10/M, 8,000 cache reads at $0.01/M, 300 cache writes at $0.125/M, 1,200
// output at $0.50/M.
const CLAUDE_USAGE_USD = 900 * 0.10 / 1e6 + 8000 * 0.01 / 1e6 + 300 * 0.125 / 1e6 + 1200 * 0.50 / 1e6;
// The same call at claude-sonnet-5-5's (a shadow judge): $2.00, $0.20, $2.50 and $10.00 per million.
const SONNET_USAGE_USD = 900 * 2 / 1e6 + 8000 * 0.2 / 1e6 + 300 * 2.5 / 1e6 + 1200 * 10 / 1e6;

// The judge (Claude) answers `judge` (an object, or text) with `usage`, or fails with `judgeFail()`.
function claudeFake({ judge = verdict(), usage = CLAUDE_USAGE, judgeFail = null } = {}, calls = []) {
  return fakeClaude((body) => {
    if (judgeFail) return judgeFail();
    return { model: `${body.model}-20261001`, stop_reason: 'end_turn', stop_details: null, content: [{ type: 'text', text: typeof judge === 'string' ? judge : JSON.stringify(judge) }], ...(usage ? { usage } : {}) };
  }, calls);
}

// The researcher (Grok) answers one finding, with the usage xAI reports.
const researcherFake = (calls = []) => async (body) => {
  calls.push(JSON.parse(JSON.stringify(body)));
  return response({ model: 'grok-4.7-0925', usage: XAI_USAGE, output: [{ type: 'message', content: [{ type: 'output_text', text: '{"findings":[{"fact":"Port of Tacoma","finding":"A public container port in Washington.","source_url":"https://www.portoftacoma.com/"}]}', annotations: [{ type: 'url_citation', url: 'https://www.portoftacoma.com/' }] }] }] });
};

function deps({ jev = jevAnswers(), jevImpl = null, grok = null, judge, usage, judgeFail } = {}) {
  const calls = { jev: [], judge: [], grok: [] };
  return {
    calls,
    verifyAuth: async request => (request.headers.get('Authorization') || '').replace('Bearer ', '') || null,
    requireAdmin: async request => ((request.headers.get('Authorization') || '').replace('Bearer ', '') === 'admin-1' ? 'admin-1' : null),
    json, cors: {},
    auditLog: async () => {},
    claude: claudeFake({ judge, usage, judgeFail }, calls.judge),
    grokFetch: grok ? grok(calls.grok) : researcherFake(calls.grok),
    grokModel: 'grok-4.7',
    policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED,
    fetch: jevImpl ? jevImpl(calls.jev) : jevFetch(jev, calls.jev),
  };
}

const SIM = 'a1b2c3d4-0000-4000-8000-000000000001';
const dossier = (over = {}) => ({
  dossier_version: 1, content_root: 'blake3:abc',
  listing: { name: 'Harbor Town' },
  digest: { entity_count: 420, class_histogram: [{ class_name: 'Part', count: 300 }], default_material_fraction: 0.3, default_name_fraction: 0.2, duplicate_transform_fraction: 0.05, script_count: 3, text_string_count: 12, spawn_count: 2 },
  strings: ['Harbor Town', 'Welcome to the docks', 'Fish market'],
  scripts: [{ path: 'Soul/tide/tide.luau', language: 'luau', lines: 40, source: 'local tide = 0\nwhile true do tide = tide + 1 end' }],
  assets: { sounds: ['gulls.ogg'] }, signals: { urls: [], emails: [], phones: [], discord_invites: [] },
  ...over,
});
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);

const TWIN_TEXT = 'Category: Digital twin\nObjective: Inspect how the Port of Tacoma stages containers.\nLesson: How a terminal moves cargo from ship to rail.\nReal-world link: A twin of the Port of Tacoma terminal from public aerial imagery.';

async function publishedSim(env, { id = SIM, author = 'user-1', withDossier = dossier(), withCaptures = 2, pak = 'PAKBYTES-1', name = 'Harbor Town', description = `A working harbor with a tide simulation.\n${STATEMENT_TEXT}` } = {}) {
  const sim = { id, name, description, genre: 'Simulation', author_id: author, author_name: 'ann', is_public: true, thumbnail_url: null, r2_key: `universes/${id}/universe.pak`, play_count: 0, published_at: new Date().toISOString(), updated_at: new Date().toISOString(), moderation: { status: 'pending' }, rating_questionnaire: questionnaireRecord(questionnaireAnswers()) };
  const stored = await env.SCENES.put(sim.r2_key, new TextEncoder().encode(pak).buffer, { sha256: createHash('sha256').update(pak).digest('hex') });
  sim.pak_etag = stored.etag; sim.scene_size_bytes = stored.size;
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  if (withDossier) await env.SCENES.put(`universes/${id}/moderation/dossier.json`, JSON.stringify(withDossier), { httpMetadata: { contentType: 'application/json' } });
  for (let n = 0; n < withCaptures; n++) await env.SCENES.put(`universes/${id}/moderation/capture-${n}.png`, PNG.buffer, { httpMetadata: { contentType: 'image/png' }, sha256: createHash('sha256').update(PNG).digest('hex') });
  return sim;
}

const ledgerOf = async (env, id = SIM) => JSON.parse(await env.SOCIAL.get(costLedgerKey(id)));
const today = () => new Date().toISOString().slice(0, 10);
const close = (a, b, msg) => assert.ok(a !== null && Math.abs(a - b) < 1e-9, `${msg}: ${a} != ${b}`);

// Calls, built the way the meter records them, and the lines and aggregates built from them.
const PRICES = reviewPricesFrom({});
const call = (model, usage, images = 0) => ({ model, usage: normalizeUsage(usage), images });
function line({ k = 'initial', r = 'publish', mb = 1, reviewedMb = 0, calls = [], prices = PRICES }) {
  const m = {};
  for (const c of calls) {
    const v = callVector(c, prices);
    const acc = m[c.model] ||= zeroVector();
    for (let i = 0; i < v.length; i++) acc[i] += v[i];
  }
  return { v: 1, k, r, done: true, pb: Math.round(mb * BYTES_PER_MB), db: Math.round(reviewedMb * BYTES_PER_MB), cb: 0, cn: 0, sb: 0, m };
}

// ── usage at every call ─────────────────────────────────────────────────────

test('usage is recorded at every call: Jev, the researcher, the judge and the shadow, with the model sent, images, latency and bytes', async () => {
  // A Digital twin naming a real place: Jev, then the researcher, then the judge, with a shadow judge beside it.
  const env = environment({ JUDGE_SHADOW_MODEL: 'claude-sonnet-5-5' });
  const d = deps();
  await publishedSim(env, { description: TWIN_TEXT });
  const rec = await runModerationCase(SIM, env, d);
  assert.equal(rec.status, 'approved');
  const [run] = (await ledgerOf(env)).runs;
  assert.equal(run.kind, 'initial');
  assert.equal(run.reason, 'publish');
  // The judge and the shadow run at the same time, so their order in the ledger is whichever answered first.
  assert.deepEqual(run.calls.map(c => c.stage).sort(), ['jev', 'judge', 'researcher', 'shadow']);
  assert.deepEqual(run.calls.slice(0, 2).map(c => c.stage), ['jev', 'researcher'], 'the researcher runs before the judge');

  const by = Object.fromEntries(run.calls.map(c => [c.stage, c]));
  const { jev, judge, researcher, shadow } = by;
  assert.equal(researcher.model, 'grok-4.7', 'the model the Grok client resolved');
  assert.equal(researcher.transport, 'xai');
  assert.equal(researcher.images, 0, 'the researcher is never sent an image');
  close(researcher.cost_usd, XAI_USAGE_USD, 'the researcher at grok-4.7\'s price');
  assert.equal(shadow.model, 'claude-sonnet-5-5');
  assert.equal(shadow.transport, 'anthropic');
  close(shadow.cost_usd, SONNET_USAGE_USD, 'the shadow at its own price');
  assert.equal(jev.model, JEV_MODEL, 'the model string Jev was sent');
  assert.equal(d.calls.jev[0].model, jev.model);
  assert.equal(jev.transport, 'typesafe');
  assert.equal(jev.ok, true);
  assert.deepEqual(jev.usage, { input_tokens: 1234, cached_input_tokens: null, output_tokens: 0, reasoning_tokens: null, reasoning_separate: null });
  assert.deepEqual(jev.usage_raw, { input_tokens: 1234, output_tokens: 0 });
  assert.equal(jev.cost_status, 'known', 'Jev is priced at the rate TypeSafe bills');
  close(jev.cost_usd, Math.round(1234 * 0.042 / 1e6 * 1e8) / 1e8, 'input at $0.042 per million (the meter keeps 8 decimals); output is free');

  assert.equal(judge.model, 'claude-haiku-5-5');
  assert.equal(judge.model_reported, 'claude-haiku-5-5-20261001');
  assert.equal(judge.transport, 'anthropic');
  assert.equal(judge.images, 2, 'two captures, no thumbnail or cover');
  // Anthropic counts cache reads and writes apart from input_tokens; the prompt is all three.
  assert.deepEqual(judge.usage, { input_tokens: 9200, cached_input_tokens: 8000, cache_write_input_tokens: 300, output_tokens: 1200, reasoning_tokens: null, reasoning_separate: null });
  close(judge.cost_usd, CLAUDE_USAGE_USD, 'the judge call at claude-haiku-5-5\'s price, cache reads and writes included');
  for (const c of run.calls) assert.ok(Number.isInteger(c.latency_ms) && c.latency_ms >= 0, `${c.stage} latency`);

  const stored = await env.SCENES.get(`universes/${SIM}/moderation/dossier.json`);
  assert.equal(run.bytes.package, 'PAKBYTES-1'.length);
  assert.equal(run.bytes.package_source, 'scene_size_bytes');
  assert.equal(run.bytes.dossier, stored.size);
  assert.equal(run.bytes.captures_count, 2);
  assert.equal(run.bytes.captures, 2 * PNG.byteLength);
  const sentState = JSON.stringify(d.calls.jev[0].state);
  assert.equal(run.bytes.jev_state_chars, sentState.length, 'the state Jev was actually sent');
  assert.equal(run.bytes.jev_state, Buffer.byteLength(sentState));
  assert.equal(run.totals.complete, true, 'every model in this run is priced, Jev included');
  assert.ok(Math.abs(run.totals.known_cost_usd - (XAI_USAGE_USD + CLAUDE_USAGE_USD + SONNET_USAGE_USD + 1234 * 0.042 / 1e6)) < 1e-7, 'the whole run, Jev included');
  assert.ok(run.finished_at);
});

test('Jev through the Workers AI binding is recorded under its own model string and transport', async () => {
  const env = environment({ JEV_API_KEY: undefined, AI: { run: async () => ({ ...jevAnswers(), usage: { prompt_tokens: 50, completion_tokens: 0 } }) } });
  await publishedSim(env);
  await runModerationCase(SIM, env, deps());
  const [jev] = (await ledgerOf(env)).runs[0].calls;
  assert.equal(jev.model, JEV_WORKERS_AI_MODEL);
  assert.equal(jev.transport, 'workers-ai');
  assert.equal(jev.usage.input_tokens, 50);
  assert.equal(jev.cost_status, 'price_unknown');
});

test('a call that reports no usage is recorded as null, never as a count', async () => {
  // The judge reports no usage.
  let env = environment();
  await publishedSim(env);
  await runModerationCase(SIM, env, deps({ usage: null }));
  let judge = (await ledgerOf(env)).runs[0].calls.find(c => c.stage === 'judge');
  assert.equal(judge.ok, true);
  assert.equal(judge.usage, null);
  assert.equal(judge.usage_raw, null);
  assert.equal(judge.cost_usd, null);
  assert.equal(judge.cost_status, 'usage_not_reported');
  const lineKey = [...env.SOCIAL.m.keys()].find(k => k.startsWith('modcost:run:'));
  const v = JSON.parse(env.SOCIAL.m.get(lineKey)).m['claude-haiku-5-5'];
  assert.equal(v[0] + v[1] + v[2] + v[10], 0, 'no token is counted for it');
  assert.deepEqual([v[8], v[9]], [1, 1], 'it is counted as a call without input or output tokens');

  // The judge answers 500, and then times out.
  for (const [judgeFail, status, error] of [
    [() => { throw apiError(500, 'down'); }, 500, 'http_500'],
    [() => { throw Object.assign(new Error('timed out'), { name: 'TimeoutError' }); }, null, 'timeout'],
  ]) {
    env = environment();
    await publishedSim(env);
    const rec = await runModerationCase(SIM, env, deps({ judgeFail }));
    assert.equal(rec.status, 'held');
    judge = (await ledgerOf(env)).runs[0].calls.find(c => c.stage === 'judge');
    assert.deepEqual([judge.ok, judge.status, judge.error, judge.usage], [false, status, error, null]);
  }

  // Jev is rate limited once, then answers: both attempts are calls, and the refused one has no usage.
  env = environment();
  await publishedSim(env);
  const jevImpl = calls => async (url, init) => {
    calls.push(JSON.parse(init.body));
    return calls.length === 1 ? new Response('slow down', { status: 429 }) : new Response(JSON.stringify(jevAnswers()), { status: 200 });
  };
  await runModerationCase(SIM, env, deps({ jevImpl }));
  const jevCalls = (await ledgerOf(env)).runs[0].calls.filter(c => c.stage === 'jev');
  assert.deepEqual(jevCalls.map(c => [c.attempt, c.ok, c.status, c.usage === null]), [[1, false, 429, true], [2, true, 200, false]]);
});

test('the meter records nothing it was not given: an empty usage object is null, a partial one keeps its null', () => {
  assert.equal(normalizeUsage(null), null);
  assert.equal(normalizeUsage({}), null);
  assert.equal(normalizeUsage({ total_tokens: 12 }), null, 'a total alone is not an input or an output count');
  assert.deepEqual(normalizeUsage({ input_tokens: 10 }), { input_tokens: 10, cached_input_tokens: null, output_tokens: null, reasoning_tokens: null, reasoning_separate: null });
  const meter = createReviewMeter({}, SIM, 'publish');
  meterCall(meter, { stage: 'judge', model: 'grok-4.6', usage: undefined });
  assert.equal(meter.calls[0].usage, null);
  assert.equal(createReviewMeter({ REVIEW_METERING: 'off' }, SIM, 'publish'), null, 'metering can be switched off');
});

// ── prices ──────────────────────────────────────────────────────────────────

test('the price table is keyed by the model strings the Worker sends, and holds no guessed price', () => {
  assert.ok(Object.hasOwn(DEFAULT_REVIEW_PRICES, JEV_MODEL));
  assert.ok(Object.hasOwn(DEFAULT_REVIEW_PRICES, JEV_WORKERS_AI_MODEL));
  // The model every xAI call sends by default (src/grok_model.mjs) has an entry with a cited source.
  assert.ok(Object.hasOwn(DEFAULT_REVIEW_PRICES, DEFAULT_GROK_MODEL), `${DEFAULT_GROK_MODEL} has a price entry`);
  assert.ok(DEFAULT_REVIEW_PRICES[DEFAULT_GROK_MODEL].input_per_mtok === null || typeof DEFAULT_REVIEW_PRICES[DEFAULT_GROK_MODEL].source === 'string', 'a price is cited or left unknown');
  const grokModel = 'grok-4.6';
  // Jev over TypeSafe is priced at the rate McKale gave (input $0.042 per million, output free), and cited. Jev over Workers AI
  // stays unknown: nothing records its rate.
  assert.equal(DEFAULT_REVIEW_PRICES[JEV_MODEL].input_per_mtok, 0.042);
  assert.equal(DEFAULT_REVIEW_PRICES[JEV_MODEL].output_per_mtok, 0);
  assert.match(DEFAULT_REVIEW_PRICES[JEV_MODEL].source, /McKale/);
  assert.equal(DEFAULT_REVIEW_PRICES[JEV_WORKERS_AI_MODEL].input_per_mtok, null);
  assert.equal(DEFAULT_REVIEW_PRICES[JEV_WORKERS_AI_MODEL].output_per_mtok, null);
  // Grok's is the list price the cost model in the docs records, and the doc still says so.
  const doc = readFileSync(new URL('../../../../docs/architecture/MODERATION_PIPELINE.md', import.meta.url), 'utf8');
  assert.match(doc, /Grok 4\.6 at \$2\.00 per million input tokens, \$0\.50 cached, \$6\.00 per million output, doubled for any single prompt at or above 200K tokens/);
  const g = DEFAULT_REVIEW_PRICES[grokModel];
  assert.deepEqual([g.input_per_mtok, g.cached_input_per_mtok, g.output_per_mtok, g.long_context_tokens, g.long_context_multiplier], [2, 0.5, 6, 200_000, 2]);
  assert.match(g.source, /MODERATION_PIPELINE\.md section 9/);
  assert.equal(MAX_QUOTE_BYTES, MAX_WORLD_BYTES);
});

test('known prices give the right cost', () => {
  const cost = (usage, images = 0, prices = PRICES, model = 'grok-4.6') => callCost(call(model, usage, images), prices);
  // 80K uncached at $2/M, 20K cached at $0.50/M, 10K output at $6/M; images are billed as tokens, not again.
  const c = cost({ input_tokens: 100_000, input_tokens_details: { cached_tokens: 20_000 }, output_tokens: 10_000 }, 4);
  assert.equal(c.status, 'known');
  close(c.usd, 0.16 + 0.01 + 0.06, 'cached, uncached and output');
  // At or past 200K input, the whole call at twice the rate.
  close(cost({ input_tokens: 1_000_000, input_tokens_details: { cached_tokens: 200_000 }, output_tokens: 10_000 }).usd, (1.6 + 0.1 + 0.06) * 2, 'long context');
  close(cost({ input_tokens: 199_999, output_tokens: 0 }).usd, 199_999 * 2 / 1e6, 'just under it');
  // A call that does not say what was cached is priced as if nothing was.
  close(cost({ input_tokens: 100_000, output_tokens: 0 }).usd, 0.2, 'no cached count');
  // Reasoning counted outside the output (the total says so) is billed as output; counted inside, it is not added twice.
  close(cost({ input_tokens: 100, output_tokens: 10, output_tokens_details: { reasoning_tokens: 50 }, total_tokens: 160 }).usd, (100 * 2 + 60 * 6) / 1e6, 'reasoning outside');
  close(cost({ input_tokens: 100, output_tokens: 60, output_tokens_details: { reasoning_tokens: 50 }, total_tokens: 160 }).usd, (100 * 2 + 60 * 6) / 1e6, 'reasoning inside');
  // A flat price per call needs no usage.
  const flat = { flat: { billed_by: 'call', per_call: 0.002 } };
  assert.deepEqual(callCost({ model: 'flat', usage: null, images: 0 }, flat), { usd: 0.002, status: 'known' });
  // A price set through MODERATION_PRICES, cited as set there.
  const set = reviewPricesFrom({ MODERATION_PRICES: '{"jev-latest":{"input_per_mtok":0.05,"output_per_mtok":0},"grok-4.6":{"output_per_mtok":-1}}' });
  assert.equal(set['jev-latest'].source, 'MODERATION_PRICES');
  close(cost({ input_tokens: 1234, output_tokens: 0 }, 0, set, 'jev-latest').usd, 1234 * 0.05 / 1e6, 'Jev once priced');
  assert.equal(set['grok-4.6'].output_per_mtok, 6, 'a negative price is refused');
  assert.deepEqual(reviewPricesFrom({ MODERATION_PRICES: 'not json' }), PRICES, 'bad JSON keeps the defaults');
  // A model with no entry has no cost, and nothing called costs nothing.
  assert.deepEqual(cost({ input_tokens: 10, output_tokens: 1 }, 0, PRICES, 'someone-else'), { usd: null, status: 'no_price' });
});

// ── the quote ───────────────────────────────────────────────────────────────

// A price table with Jev known, for quotes that can complete.
// Jev with its price set back to unknown, for the incomplete-quote path.
const UNPRICED_JEV = reviewPricesFrom({ MODERATION_PRICES: '{"jev-latest":{"input_per_mtok":null,"output_per_mtok":null,"source":"test: unpriced"}}' });
const KNOWN = reviewPricesFrom({ MODERATION_PRICES: '{"jev-latest":{"input_per_mtok":0.05,"output_per_mtok":0}}' });
const JEV_USAGE = { input_tokens: 20_000, output_tokens: 0 };
const JEV_USD = 20_000 * 0.05 / 1e6;

test('an unknown price gives an incomplete quote: no price, the reason, and the known part apart', () => {
  const lines = Array.from({ length: 30 }, () => line({ mb: 2, calls: [call('jev-latest', JEV_USAGE), call('grok-4.6', XAI_USAGE, 4)] }));
  const q = quoteReview({ sizeBytes: 2 * BYTES_PER_MB, kind: 'initial', days: [aggregateLines('2026-10-01', lines)], prices: UNPRICED_JEV });
  assert.equal(q.complete, false);
  assert.equal(q.price_usd, null);
  assert.equal(q.cost_usd, null);
  assert.deepEqual(q.reasons, [{ code: 'price_unknown', model: 'jev-latest' }]);
  assert.equal(q.basis['jev-latest'].expected_usd, null);
  assert.equal(q.basis['jev-latest'].tokens_per_review, 20_000, 'the tokens are still shown');
  close(q.known_part.cost_usd, XAI_USAGE_USD, 'the judge alone');
  assert.deepEqual(q.known_part.models, ['grok-4.6']);
  assert.equal(q.charged, false);
  // A model missing from the table, and a call that reported no usage, are incomplete the same way.
  const other = quoteReview({ sizeBytes: BYTES_PER_MB, kind: 'initial', days: [aggregateLines('2026-10-01', Array.from({ length: 30 }, () => line({ calls: [call('mystery', JEV_USAGE), call('grok-4.6', null)] })))], prices: KNOWN });
  assert.deepEqual(other.reasons, [{ code: 'no_price', model: 'mystery' }, { code: 'usage_not_reported', model: 'grok-4.6', calls: 30 }]);
  assert.equal(other.price_usd, null);
});

test('the 10% margin is applied, and the floor when it is higher', () => {
  assert.equal(DEFAULT_QUOTE_CONFIG.margin, 0.10);
  const lines = Array.from({ length: 30 }, () => line({ mb: 3, calls: [call('jev-latest', JEV_USAGE), call('grok-4.6', XAI_USAGE, 4)] }));
  const days = [aggregateLines('2026-10-01', lines)];
  const q = quoteReview({ sizeBytes: 3 * BYTES_PER_MB, kind: 'initial', days, prices: KNOWN });
  assert.equal(q.complete, true, JSON.stringify(q.reasons));
  close(q.cost_usd, JEV_USD + XAI_USAGE_USD, 'the measured cost');
  close(q.price_usd, (JEV_USD + XAI_USAGE_USD) * 1.10, 'cost plus 10%');
  close(q.margin_usd, (JEV_USD + XAI_USAGE_USD) * 0.10, 'the margin');
  assert.equal(q.margin, 0.10);
  assert.equal(q.floor_applied, false);
  const floored = quoteReview({ sizeBytes: 3 * BYTES_PER_MB, kind: 'initial', days, prices: KNOWN, config: { ...DEFAULT_QUOTE_CONFIG, floor_usd: 0.25 } });
  assert.equal(floored.price_usd, 0.25);
  assert.equal(floored.floor_applied, true);
  assert.deepEqual(quoteConfigFrom({ MODERATION_QUOTE: '{"floor_usd":0.25,"min_samples":5,"margin":-1}' }), { ...DEFAULT_QUOTE_CONFIG, floor_usd: 0.25, min_samples: 5 });
});

test('below the minimum sample size the quote is incomplete and says so', () => {
  const lines = Array.from({ length: 29 }, () => line({ calls: [call('jev-latest', JEV_USAGE), call('grok-4.6', XAI_USAGE)] }));
  const q = quoteReview({ sizeBytes: BYTES_PER_MB, kind: 'initial', days: [aggregateLines('2026-10-01', lines)], prices: KNOWN });
  assert.equal(q.complete, false);
  assert.equal(q.price_usd, null);
  assert.deepEqual(q.reasons, [{ code: 'not_enough_samples', samples: 29, min_samples: 30 }]);
  assert.ok(q.known_part.cost_usd > 0, 'what the samples say is still shown, apart');
  const none = quoteReview({ sizeBytes: BYTES_PER_MB, kind: 'update', days: [], prices: KNOWN });
  assert.deepEqual([none.complete, none.price_usd, none.known_part, none.samples.reviews], [false, null, null, 0]);
});

test('an update is priced from update reviews alone, and costs less when it measures less', () => {
  // A first review reaches the judge and the agent; an update here only Jev and the judge. Admin re-runs and appeals are not
  // publishes and are left out of both.
  const initial = Array.from({ length: 30 }, () => line({ k: 'initial', calls: [call('jev-latest', JEV_USAGE), call('grok-4.6', XAI_USAGE, 4), call('grok-4.6', XAI_USAGE)] }));
  const update = Array.from({ length: 30 }, (_, i) => line({ k: 'update', r: i % 2 ? 'publish' : 'sweep', calls: [call('jev-latest', JEV_USAGE), call('grok-4.6', XAI_USAGE, 4)] }));
  const noise = [line({ k: 'update', r: 'rerun', calls: Array(9).fill(call('grok-4.6', XAI_USAGE)) }), line({ k: 'appeal', r: 'appeal', calls: Array(9).fill(call('grok-4.6', XAI_USAGE)) })];
  const days = [aggregateLines('2026-10-01', [...initial, ...noise]), aggregateLines('2026-10-02', update)];
  const qi = quoteReview({ sizeBytes: BYTES_PER_MB, kind: 'initial', days, prices: KNOWN });
  const qu = quoteReview({ sizeBytes: BYTES_PER_MB, kind: 'update', days, prices: KNOWN });
  assert.equal(qu.complete, true);
  assert.equal(qu.samples.reviews, 30);
  close(qu.cost_usd, JEV_USD + XAI_USAGE_USD, 'the update reviews\' own average');
  close(qi.cost_usd, JEV_USD + 2 * XAI_USAGE_USD, 'the first reviews\' own average');
  assert.ok(qu.price_usd < qi.price_usd);
  // More first reviews do not move the update's price.
  const more = [...days, aggregateLines('2026-10-03', Array.from({ length: 50 }, () => line({ k: 'initial', calls: Array(4).fill(call('grok-4.6', XAI_USAGE)) })))];
  assert.equal(quoteReview({ sizeBytes: BYTES_PER_MB, kind: 'update', days: more, prices: KNOWN }).price_usd, qu.price_usd);
});

test('$/MB is right on a fixture: the fixed part and the per-MB part, and tokens per MB reviewed', () => {
  // Grok's uncached input grows by 500 tokens a MB from 1,000; 100 output tokens a call. So a review costs
  // (1000 + 500x) * $2/M + 100 * $6/M = $0.0026 + $0.001 per MB.
  const at = x => line({ mb: x, reviewedMb: 0.5, calls: [call('grok-4.6', { input_tokens: 1000 + 500 * x, output_tokens: 100 })] });
  const days = [aggregateLines('2026-10-01', [at(1), at(2), at(3), at(4)])];
  const q = quoteReview({ sizeBytes: 10 * BYTES_PER_MB, kind: 'initial', days, prices: PRICES, config: { ...DEFAULT_QUOTE_CONFIG, min_samples: 4 } });
  assert.equal(q.complete, true, JSON.stringify(q.reasons));
  close(q.basis['grok-4.6'].fixed_usd, 0.0026, 'the fixed part');
  close(q.basis['grok-4.6'].per_mb_usd, 0.001, 'the per-MB part');
  close(q.cost_usd, 0.0026 + 0.001 * 10, 'a 10 MB Space');
  close(q.price_usd, (0.0026 + 0.001 * 10) * 1.1, 'with the margin');
  assert.equal(q.samples.size_in_range, false, 'past the sizes measured, and it says so');
  assert.equal(q.basis['grok-4.6'].tokens_per_mb, 500);

  const r = reviewCostMetrics(days, PRICES);
  // 4 reviews, 10 MB of packages, 2 MB of dossier; 9,400 tokens; $0.0204.
  assert.equal(r.mb.package, 10);
  assert.equal(r.mb.reviewed, 2);
  assert.equal(r.models['grok-4.6'].tokens.total, 9400);
  assert.equal(r.models['grok-4.6'].tokens_per_package_mb, 940);
  assert.equal(r.models['grok-4.6'].tokens_per_reviewed_mb, 4700);
  close(r.models['grok-4.6'].usd_per_package_mb, 0.00204, '$ per package MB');
  close(r.models['grok-4.6'].usd_per_reviewed_mb, 0.0102, '$ per MB reviewed');
  close(r.overall.usd_per_package_mb, 0.00204, 'overall');
  // Average and spread of a first review: $0.0036, 0.0046, 0.0056, 0.0066.
  close(r.overall.initial.average_usd, 0.0051, 'average');
  close(r.overall.initial.median_usd, 0.0051, 'median');
  close(r.overall.initial.p90_usd, 0.0066, '90th percentile');
  assert.equal(r.overall.update.reviews, 0);
  // A slope that would make a bigger Space cheaper is taken as flat.
  const falling = [aggregateLines('2026-10-01', [1, 2, 3, 4].map(x => line({ mb: x, calls: [call('grok-4.6', { input_tokens: 5000 - 1000 * x, output_tokens: 0 })] })))];
  const fq = quoteReview({ sizeBytes: 100 * BYTES_PER_MB, kind: 'initial', days: falling, prices: PRICES, config: { ...DEFAULT_QUOTE_CONFIG, min_samples: 4 } });
  assert.equal(fq.basis['grok-4.6'].per_mb_usd, 0);
  close(fq.cost_usd, 2500 * 2 / 1e6, 'the average');
});

// ── the ledger and the aggregates ───────────────────────────────────────────

async function meteredRun(env, { simId, day, kind = 'initial', reason = 'publish', pkg = 3 * BYTES_PER_MB, calls }) {
  const meter = createReviewMeter(env, simId, reason, Date.parse(`${day}T12:00:00Z`));
  meterKind(meter, kind);
  meterBytes(meter, { package: pkg, package_source: 'scene_size_bytes', dossier: 1000, captures_count: 4, captures: 4000 });
  for (const c of calls) meterCall(meter, { stage: c.stage || 'judge', model: c.model, transport: 'xai', ok: true, usage: c.usage, images: c.images || 0 });
  return persistReviewMeter(env, meter);
}

test('the aggregates add up across days, and match the case ledgers', async () => {
  const env = environment({ MODERATION_PRICES: '{"jev-latest":{"input_per_mtok":0.05,"output_per_mtok":0}}' });
  const prices = reviewPricesFrom(env);
  const ids = n => `d0000000-0000-4000-8000-00000000000${n}`;
  const plan = [
    { simId: ids(1), day: '2026-10-01', calls: [{ model: 'jev-latest', usage: JEV_USAGE, stage: 'jev' }, { model: 'grok-4.6', usage: XAI_USAGE, images: 4 }] },
    { simId: ids(2), day: '2026-10-01', calls: [{ model: 'jev-latest', usage: JEV_USAGE, stage: 'jev' }] },
    { simId: ids(1), day: '2026-10-02', kind: 'update', calls: [{ model: 'jev-latest', usage: JEV_USAGE, stage: 'jev' }, { model: 'grok-4.6', usage: XAI_USAGE, images: 4 }, { model: 'grok-4.6', usage: XAI_USAGE }] },
    { simId: ids(3), day: '2026-10-03', reason: 'rerun', calls: [{ model: 'grok-4.6', usage: XAI_USAGE }] },
  ];
  for (const p of plan) await meteredRun(env, p);
  for (const day of ['2026-10-01', '2026-10-02', '2026-10-03']) assert.ok(await rollupReviewCosts(env, day));
  assert.equal(await rollupReviewCosts(env, '2026-10-04'), null, 'a day with no runs writes nothing');
  assert.equal(await env.SOCIAL.get(costDayKey('2026-10-04')), null);

  const read = async (from, to) => (await reviewCostReport(env, new URLSearchParams({ from, to }))).body;
  const all = await read('2026-10-01', '2026-10-03');
  const each = await Promise.all(['2026-10-01', '2026-10-02', '2026-10-03'].map(d => read(d, d)));
  assert.equal(all.runs, 4);
  assert.equal(all.runs, each.reduce((s, r) => s + r.runs, 0));
  for (const model of ['jev-latest', 'grok-4.6']) {
    assert.equal(all.models[model].calls, each.reduce((s, r) => s + (r.models[model]?.calls || 0), 0), `${model} calls`);
    assert.equal(all.models[model].tokens.total, each.reduce((s, r) => s + (r.models[model]?.tokens.total || 0), 0), `${model} tokens`);
    close(all.models[model].cost_usd, each.reduce((s, r) => s + (r.models[model]?.cost_usd || 0), 0), `${model} dollars`);
  }
  assert.equal(all.mb.package, 12);
  assert.deepEqual(all.days.map(d => [d.day, d.runs]), [['2026-10-01', 2], ['2026-10-02', 1], ['2026-10-03', 1]]);
  // The admin rerun is reported under its reason and kept out of the reviews a publish is priced by.
  assert.equal(all.reasons.rerun.runs, 1);
  assert.equal(all.overall.initial.reviews, 2);
  assert.equal(all.overall.update.reviews, 1);
  // The books: the case ledgers' totals add up to the same dollars.
  let ledgers = 0;
  for (const n of [1, 2, 3]) for (const run of (await ledgerOf(env, ids(n))).runs) ledgers += run.totals.cost_usd;
  close(all.overall.cost_usd, ledgers, 'aggregates and ledgers');
  close(all.overall.cost_usd, 3 * JEV_USD + 4 * XAI_USAGE_USD, 'by hand');
  assert.equal((await ledgerOf(env, ids(1))).runs.length, 2, 'one ledger per case, a run per review');
  // Merging the days is the same as rolling all their lines up together.
  const lines = [...env.SOCIAL.m.keys()].filter(k => k.startsWith('modcost:run:')).map(k => JSON.parse(env.SOCIAL.m.get(k)));
  const sums = r => Object.fromEntries(Object.entries(r.models).map(([m, x]) => [m, [x.calls, x.tokens, x.cost_usd]]));
  assert.deepEqual(sums(reviewCostMetrics([aggregateLines('x', lines)], prices)), sums(reviewCostMetrics([mergeAggregates(['2026-10-01', '2026-10-02', '2026-10-03'].map(d => JSON.parse(env.SOCIAL.m.get(costDayKey(d)))))], prices)));
});

test('a rollup without list metadata reads the lines themselves, and the hourly sweep rolls up today', async () => {
  const env = environment({ SOCIAL: kv({ metadata: false }) });
  await publishedSim(env);
  await runModerationCase(SIM, env, deps());
  const out = await sweepModeration(env, deps(), { backfill: false });
  assert.equal(out.cost_days, 1);
  const day = JSON.parse(await env.SOCIAL.get(costDayKey(today())));
  assert.equal(day.runs, 1);
  assert.equal(day.models['claude-haiku-5-5'][4], 1, 'one judge call');
  assert.equal(day.groups.initial.runs, 1);
});

test('an appeal calls no model (a person reviews it), and an update is metered as an update: neither is priced as a first review', async () => {
  const env = environment();
  const d = deps({ judge: verdict({ verdict: 'reject', articles: [CITE_A4] }) });
  await publishedSim(env);
  assert.equal((await runModerationCase(SIM, env, d)).status, 'rejected');
  const appeal = new Request(`https://api.eustress.dev/api/simulations/${SIM}/appeal`, { method: 'POST', headers: { Authorization: 'Bearer user-1' }, body: JSON.stringify({ text: 'Please look again at the tide model and its gauge.' }) });
  const res = await handleModerationRoute(appeal, new URL(appeal.url), env, null, d);
  assert.equal(res.status, 202);
  // The author changes the Space and publishes again: a review of a listing decided before.
  await publishedSim(env, { pak: 'PAKBYTES-2-CHANGED' });
  await runModerationCase(SIM, env, d);
  const runs = (await ledgerOf(env)).runs;
  assert.deepEqual(runs.map(r => [r.kind, r.reason, r.calls.map(c => c.stage).join('+')]), [['initial', 'publish', 'jev+judge'], ['update', 'publish', 'jev+judge']]);
  await rollupReviewCosts(env, today());
  const report = (await reviewCostReport(env, new URLSearchParams())).body;
  assert.equal(report.reasons.appeal, undefined, 'the appeal went to the Review Workspace and spent nothing');
  assert.deepEqual([report.overall.initial.reviews, report.overall.update.reviews], [1, 1]);
});

test('a ledger that cannot be saved leaves the review as it was', async () => {
  const env = environment();
  const put = env.SOCIAL.put;
  env.SOCIAL.put = async (k, ...rest) => { if (k.startsWith('modcost:')) throw new Error('KV is down'); return put(k, ...rest); };
  await publishedSim(env);
  const rec = await runModerationCase(SIM, env, deps());
  assert.equal(rec.status, 'approved');
  assert.equal(await env.SOCIAL.get(costLedgerKey(SIM)), null);
});

// ── the routes ──────────────────────────────────────────────────────────────

const get = (path, token) => new Request(`https://api.eustress.dev${path}`, { headers: token ? { Authorization: `Bearer ${token}` } : {} });
const route = async (env, path, token) => {
  const res = await handleModerationRoute(get(path, token), new URL(`https://api.eustress.dev${path}`), env, null, deps());
  return { status: res.status, body: await res.json(), headers: res.headers };
};

test('the admin cost report is for administrators only, and answers per model and overall', async () => {
  const env = environment();
  await publishedSim(env);
  await runModerationCase(SIM, env, deps());
  await rollupReviewCosts(env, today());
  assert.equal((await route(env, '/api/admin/moderation/costs')).status, 403);
  assert.equal((await route(env, '/api/admin/moderation/costs', 'user-1')).status, 403);
  const ok = await route(env, '/api/admin/moderation/costs', 'admin-1');
  assert.equal(ok.status, 200);
  assert.equal(ok.body.runs, 1);
  assert.equal(ok.body.charged, false);
  assert.equal(ok.body.models['claude-haiku-5-5'].calls, 1);
  assert.equal(ok.body.models['jev-latest'].cost_status, 'known', 'Jev is priced');
  assert.equal(typeof ok.body.overall.complete, 'boolean');
  assert.equal((await route(env, '/api/admin/moderation/costs?from=2026-01-01&to=2026-12-31', 'admin-1')).status, 400, 'at most 92 days');
  assert.equal((await route(env, '/api/admin/moderation/costs?from=yesterday', 'admin-1')).status, 400);
  assert.equal((await route(env, '/api/admin/moderation/costs?to=2026-13-45', 'admin-1')).status, 400);
});

test('the quote is session-gated, checks its input, and is rate-limited per account', async () => {
  const env = environment({ MODERATION_QUOTE: '{"quotes_per_hour":2}' });
  const path = `/api/simulations/review-quote?size_bytes=${3 * BYTES_PER_MB}&kind=update`;
  assert.equal((await route(env, path)).status, 401);
  assert.equal((await route(env, '/api/simulations/review-quote?size_bytes=0', 'user-1')).status, 400);
  assert.equal((await route(env, `/api/simulations/review-quote?size_bytes=${MAX_QUOTE_BYTES + 1}`, 'user-1')).status, 400);
  assert.equal((await route(env, '/api/simulations/review-quote?size_bytes=12&kind=appeal', 'user-1')).status, 400);
  const first = await route(env, path, 'user-1');
  assert.equal(first.status, 200);
  assert.deepEqual([first.body.kind, first.body.complete, first.body.price_usd, first.body.charged], ['update', false, null, false]);
  assert.equal((await route(env, path, 'user-1')).status, 200);
  const third = await route(env, path, 'user-1');
  assert.equal(third.status, 429);
  assert.equal(third.headers.get('Retry-After'), '3600');
  assert.equal((await route(env, path, 'user-2')).status, 200, 'another account has its own allowance');
});

test('the real Worker routes both: no session is 401, a member is 403 on the admin report', async () => {
  const worker = await loadWorker();
  const env = fakeEnv();
  const send = (path, token) => worker.fetch(get(path, token), env, ctx);
  assert.equal((await send(`/api/simulations/review-quote?size_bytes=1024`)).status, 401);
  const quote = await send(`/api/simulations/review-quote?size_bytes=1024&kind=initial`, mintToken('member-1'));
  assert.equal(quote.status, 200);
  assert.equal((await quote.json()).complete, false);
  assert.equal((await send('/api/admin/moderation/costs', mintToken('member-1'))).status, 403);
  await env.USERS.put('admin:admin-9', '1');
  const report = await send('/api/admin/moderation/costs', mintToken('admin-9'));
  assert.equal(report.status, 200);
  assert.equal((await report.json()).runs, 0);
});

// ── metering never changes a decision ───────────────────────────────────────
// Every decision path the moderation tests drive, run twice from a fresh start: once with metering off, once with it on. The
// case, the listing, every request each model was sent, and every key outside the meter's own must come out the same.

const TIME = /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/g;
const MS = /"(?:jev_ms|to_triage_ms|judge_ms|total_ms|ms)":\d+/g;
function normalized(value) {
  return JSON.parse(JSON.stringify(value ?? null, (k, v) => {
    if (k === 'timing' || k === 'ms' || k === 'classifying_since' || k === 'at' || /_at$/.test(k)) return undefined;
    if (k === 'ai_verdicts') return Array.isArray(v) ? v.length : v;
    // A case reference is 100 random bits (src/case_reference.mjs): present or not is what the two runs share.
    if (k === 'reference') return v ? '<reference>' : v;
    if (typeof v === 'string') return v.replace(TIME, '<time>').replace(MS, '"ms":0');
    return v;
  }));
}

const FIXTURES = {
  listed: {},
  'quarantined at triage': { jev: { csam_or_minor_sexualization: 0.95 } },
  'rejected in the harm lane': { jev: { real_crime_instructions: 0.9, real_world_intent: 0.9 } },
  'held for a sensitive real place': { jev: { sensitive_real_place: 0.7 } },
  'changes requested for a child-directed Space': { jev: { child_directed: 0.9, external_links_or_contact: 0.9 } },
  'rejected as empty': { sim: { withDossier: dossier({ digest: { entity_count: 0 } }) } },
  'rejected by the judge': { judge: verdict({ verdict: 'reject', articles: [CITE_A4] }) },
  'held: the judge flagged it': { judge: verdict({ verdict: 'flag_for_human_review', confidence: 0.3 }) },
  'held: the judge answered no JSON': { judge: 'not json' },
  'held: the judge answered 500': { judgeFail: () => { throw apiError(500, 'down'); } },
  'held: Jev is down': { jevImpl: () => async () => new Response('down', { status: 500 }) },
  'escalated: held for a person': { jev: { real_crime_instructions: 0.5 } },
  'researched: a Digital twin of a real place': { sim: { description: TWIN_TEXT } },
  'researched: the researcher failed': { sim: { description: TWIN_TEXT }, grok: () => async () => response({ error: 'down' }, 503) },
  'listed with a shadow judge beside it': { env: { JUDGE_SHADOW_MODEL: 'claude-sonnet-5-5' } },
  'listed and audited': { env: { MODERATION_THRESHOLDS: '{"audit_rate":1}' } },
  'no usage reported': { usage: null },
  deduplicated: { run: async (env, d) => {
    const other = 'b1b2c3d4-0000-4000-8000-000000000002';
    await publishedSim(env, { id: other });
    await runModerationCase(other, env, d);
    await publishedSim(env);
    await runModerationCase(SIM, env, d);
  } },
  'an update after a content change': { run: async (env, d) => {
    await publishedSim(env);
    await runModerationCase(SIM, env, d);
    await publishedSim(env, { pak: 'PAKBYTES-2-CHANGED' });
    await runModerationCase(SIM, env, d);
  } },
};

async function runFixture(fx, metering) {
  const env = environment({ ...(fx.env || {}), ...(metering ? {} : { REVIEW_METERING: 'off' }) });
  const d = deps({ jev: jevAnswers(fx.jev || {}), jevImpl: fx.jevImpl, grok: fx.grok, judge: fx.judge, judgeFail: fx.judgeFail, usage: fx.usage === undefined ? CLAUDE_USAGE : fx.usage });
  if (fx.run) await fx.run(env, d);
  else { await publishedSim(env, fx.sim || {}); await runModerationCase(SIM, env, d); }
  const keys = [...env.SOCIAL.m.keys()].filter(k => !k.startsWith('modcost:')).map(k => k.split(':')[0]).sort();
  return {
    env,
    seen: normalized({
      case: await loadCase(env, SIM),
      listing: JSON.parse(await env.SOCIAL.get(`sim:${SIM}`)).moderation,
      jev: d.calls.jev,
      judge: d.calls.judge.map(c => c.body),
      grok: d.calls.grok,
      keys,
      objects: [...env.SCENES.m.keys()].sort(),
    }),
  };
}

for (const [name, fx] of Object.entries(FIXTURES)) {
  test(`metering never changes a moderation decision: ${name}`, async () => {
    const off = await runFixture(fx, false);
    const on = await runFixture(fx, true);
    assert.deepEqual(on.seen, off.seen);
    assert.equal(await off.env.SOCIAL.get(costLedgerKey(SIM)), null, 'the control ran with metering off');
    const status = off.seen.case?.status;
    // A quarantine, a hold or a review that ran is metered; only the control is not.
    assert.ok(await on.env.SOCIAL.get(costLedgerKey(SIM)) || status === 'quarantined', `${name}: metered (${status})`);
  });
}
