import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import {
  buildJevQuestions, buildJevState, jevEvaluate, deterministicSignals,
  decideTriage, decideFinal, parseJudgeVerdict, runModerationCase, makeToolExecutor,
  handleModerationRoute, isListable, canServe, publicModeration, loadCase,
  DEFAULT_THRESHOLDS, HARD_CATEGORIES, SOFT_CATEGORIES, QUALITY_BANDS, RATINGS,
  MODERATION_TOOLS, AGENT_TOOL_NAMES, POLICY_HASH_ANCHORED, DOSSIER_MAX_BYTES, ARTICLE_SIGNALS, JEV_STATE_MAX_CHARS,
  authorView, authorStatusOf, safeAuthorText, AUTHOR_COPY, handleNotificationRoute, unsubscribeToken,
  sweepModeration, resumeStalledPending, sweepLocks, acquireLease, releaseLease, contentFingerprint, contentLockedFor, saveCase, LegalLockConflict, coverApproved, approvedCaptureKeys, simForServing, thumbnailApproved, sourceApproved, approvalMovement,
} from '../src/moderation.mjs';
import { render, TARGET } from '../scripts/sync-policy.mjs';
import { claudeFromGrok, questionnaireAnswers } from './helpers/claude_fake.mjs';
import { questionnaireRecord } from '../src/rating_questionnaire.mjs';
import { GUARDIAN_POLICY_TEXT, MODERATION_PLAYBOOK_TEXT } from '../src/generated/policy_text.mjs';

// ── fakes ───────────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(name => ({ name })), list_complete: true }),
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
      // R2's conditional put: a failed precondition stores nothing and returns null.
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
    list: async ({ prefix = '', limit = 1000, include = [] } = {}) => ({
      objects: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(key => ({ key, ...(include.includes('customMetadata') ? { customMetadata: m.get(key).customMetadata || {} } : {}) })),
    }),
  };
}

// audit_rate is pinned to 0 so a 3% random audit cannot add a second Grok call
// to a test that counts them; the audit path has its own test with the rate at 1.
function environment(extra = {}) {
  return { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2(), JEV_API_KEY: 'jev-test', GROK_API_KEY: 'grok-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', ...extra };
}

const json = (value, status, headers) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });

// A Jev answer set where everything is clean, then overrides.
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
    calls.push({ url, body: JSON.parse(init.body) });
    return new Response(JSON.stringify(typeof answers === 'function' ? answers(calls.length) : answers), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
}

// A Guardian Code v2.1 Part VII answer that lists a Space: no article, every test at 2, the statement matches. `scores`
// overrides test scores by name.
function verdict(v = {}) {
  const { scores = {}, ...rest } = v;
  const tests = Object.fromEntries(['build_quality', 'objective', 'lesson', 'real_world'].map(k => [k, { score: scores[k] ?? 2, evidence: `${k}: the tide gauge over the harbour basin in capture-1` }]));
  return { verdict: 'publish', articles: [], tests, statement_check: 'matches', rating_floor: 'all_ages', descriptors: [], is_game: false, is_fiction: false, featured_candidate: false, policy_version: '2.2', policy_hash: 'sha256:x', spatial_evidence: ['central plaza with four symmetric fountains', 'material variety across the arcade facades'], rationale: 'Deliberate composition, no prohibited category.', confidence: 0.9, flagged_for_review: false, ...rest };
}
// A judge rejection is a Group A citation (Code A4 here); a `reject` that cites none is malformed.
const CITE_A4 = { id: 'A4', confidence: 0.9, evidence: 'Script Soul/lab/recipe.luau lists reagent quantities and steps' };
const rejectVerdict = (v = {}) => verdict({ verdict: 'reject', articles: [CITE_A4], ...v });
// The four labelled lines every listing carries (Code Part III).
const STATEMENT_TEXT = 'Category: Simulation\nObjective: Set the tide range and read the depth at the quay.\nLesson: How tidal range and basin shape set harbour depth.\nReal-world link: Models a tidal harbour from published tide tables.';

function grokJudge(text, calls = []) {
  return async body => {
    calls.push(body);
    const t = typeof text === 'function' ? text(body) : text;
    return { ok: true, status: 200, json: async () => ({ model: 'grok-test', output: [{ type: 'message', content: [{ type: 'output_text', text: typeof t === 'string' ? t : JSON.stringify(t) }] }] }), text: async () => '' };
  };
}

function extractGrokText(data) {
  for (const item of data?.output || []) for (const c of item?.content || []) if (typeof c?.text === 'string') return c.text;
  return '';
}

function deps(env, { judge = verdict(), grok, admin = null, audits = [] } = {}) {
  const judgeCalls = [];
  return {
    calls: { judge: judgeCalls },
    verifyAuth: async request => (request.headers.get('Authorization') || '').replace('Bearer ', '') || null,
    requireAdmin: async request => ((request.headers.get('Authorization') || '').replace('Bearer ', '') === admin ? admin : null),
    json, cors: {},
    auditLog: async (env, action, adminId, target, details) => { audits.push({ action, adminId, target, details }); },
    // The Claude judge answers through the same scripted function the Grok judge's tests used (helpers/claude_fake.mjs), read
    // from these deps when the judge runs, so a test that swaps `grokFetch` swaps the judge.
    grokFetch: grok || grokJudge(judge, judgeCalls),
    claude: d => claudeFromGrok(d.grokFetch),
    grokModel: 'grok-test',
    extractGrokText,
    policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED,
    fetch: jevFetch(jevAnswers()),
  };
}

const dossier = (over = {}) => ({
  dossier_version: 1, content_root: 'blake3:abc',
  listing: { name: 'Harbor Town' },
  digest: { entity_count: 420, class_histogram: [{ class_name: 'Part', count: 300 }], default_material_fraction: 0.3, default_name_fraction: 0.2, duplicate_transform_fraction: 0.05, script_count: 3, text_string_count: 12, spawn_count: 2 },
  strings: ['Harbor Town', 'Welcome to the docks', 'Fish market'],
  scripts: [{ path: 'Soul/tide/tide.luau', language: 'luau', lines: 40, source: 'local tide = 0\nwhile true do tide = tide + 1 end' }],
  assets: { sounds: ['gulls.ogg'] }, signals: { urls: [], emails: [], phones: [], discord_invites: [] },
  ...over,
});

async function publishedSim(env, { id = 'a1b2c3d4-0000-4000-8000-000000000001', author = 'user-1', withDossier = dossier(), withCaptures = 2, pak = 'PAKBYTES-1', is_public = true, name = 'Harbor Town' } = {}) {
  const sim = { id, name, description: `A working harbor with a tide simulation.\n${STATEMENT_TEXT}`, genre: 'Simulation', author_id: author, author_name: 'ann', is_public, thumbnail_url: null, r2_key: `universes/${id}/universe.pak`, play_count: 0, published_at: new Date().toISOString(), updated_at: new Date().toISOString(), moderation: { status: 'pending' }, rating_questionnaire: questionnaireRecord(questionnaireAnswers()) };
  const stored = await env.SCENES.put(sim.r2_key, new TextEncoder().encode(pak).buffer, { sha256: createHash('sha256').update(pak).digest('hex') });
  sim.pak_etag = stored.etag; sim.scene_size_bytes = stored.size;
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  if (withDossier) await env.SCENES.put(`universes/${id}/moderation/dossier.json`, JSON.stringify(withDossier), { httpMetadata: { contentType: 'application/json' } });
  const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]).buffer;
  for (let n = 0; n < withCaptures; n++) await env.SCENES.put(`universes/${id}/moderation/capture-${n}.png`, png, { httpMetadata: { contentType: 'image/png' }, sha256: createHash('sha256').update(Buffer.from(png)).digest('hex') });
  return sim;
}

const simOf = async (env, id) => JSON.parse(await env.SOCIAL.get(`sim:${id}`));

// ── policy text ships in sync with docs ─────────────────────────────────────

test('shipped policy text matches docs/ and hashes to the anchored policy hash', () => {
  assert.equal(readFileSync(TARGET, 'utf8'), render(), 'run npm run sync-policy');
  assert.equal('sha256:' + createHash('sha256').update(GUARDIAN_POLICY_TEXT).digest('hex'), POLICY_HASH_ANCHORED);
  assert.match(MODERATION_PLAYBOOK_TEXT, /Eustress Moderation Playbook/);
});

test('the playbook names every tool, and the tools only a person may use are the ones the executor keeps from anyone else', () => {
  for (const t of MODERATION_TOOLS) assert.ok(MODERATION_PLAYBOOK_TEXT.includes(`\`${t.name}\``), `playbook mentions ${t.name}`);
  const humanOnly = MODERATION_TOOLS.filter(t => !t.agent).map(t => t.name);
  assert.deepEqual(humanOnly.sort(), ['moderation_audit_resolve', 'moderation_backfill', 'moderation_bind', 'moderation_binding_report', 'moderation_forum_case', 'moderation_forum_resolve', 'moderation_release', 'moderation_rerun', 'moderation_resolve_appeal', 'moderation_workspace_queue']);
  assert.deepEqual(AGENT_TOOL_NAMES, MODERATION_TOOLS.filter(t => t.agent).map(t => t.name));
  // Code v2.1 retired the agent: the playbook says so, and no model calls a tool.
  assert.match(MODERATION_PLAYBOOK_TEXT, /Review Workspace/);
  assert.doesNotMatch(MODERATION_PLAYBOOK_TEXT, /the agent (decides|approves|takes)/i);
});

// ── question battery and state budget ───────────────────────────────────────

test('the battery covers every hard and soft category and every Code article signal with a criteria pair', () => {
  const q = buildJevQuestions();
  const codeQuestions = [...Object.keys(ARTICLE_SIGNALS), 'lawful_decision_training', 'sensitive_real_place', 'game_mechanics', 'model_implemented', 'result_readout'];
  for (const k of [...HARD_CATEGORIES, ...SOFT_CATEGORIES, ...codeQuestions]) {
    assert.equal(q[k]?.type, 'noul', k);
    assert.ok(q[k].criteria.true && q[k].criteria.false, k);
  }
  assert.deepEqual(Object.keys(q.content_rating.criteria), RATINGS);
  assert.match(q.content_rating.criteria.adult_18, /^Sexual content only/);
  assert.match(q.content_rating.criteria.mature_17, /clinical, anatomical and surgical depiction, injury in medical or crash training, and drug education/);
  assert.deepEqual(q.quality_band.criteria, QUALITY_BANDS);
  assert.ok(Object.keys(q).length >= 49);
  // The state budget leaves the margin the comment on JEV_STATE_MAX_CHARS works out: the battery and a full state stay under
  // 32K tokens at three characters a token for the state and four for the battery, with 5K tokens spare.
  assert.ok(JEV_STATE_MAX_CHARS / 3 + JSON.stringify(q).length / 4 <= 32_000 - 5_000, 'the battery grew past the state budget\'s margin');
});

test('state stays under budget: scripts round-robin, strings trimmed last', () => {
  const big = 'x'.repeat(50_000);
  const d = dossier({ scripts: [{ path: 'a.luau', language: 'luau', lines: 1, source: big }, { path: 'b.luau', language: 'luau', lines: 1, source: 'print(1)' }], strings: Array.from({ length: 1000 }, (_, i) => `string ${i}`) });
  const roomy = buildJevState({ name: 'n' }, d, { maxChars: 20_000 });
  assert.ok(JSON.stringify(roomy).length <= 20_000);
  assert.equal(roomy.scripts.length, 2, 'the small script is not crowded out by the big one');
  assert.ok(roomy.scripts[0].truncated);
  assert.equal(roomy.scripts[1].truncated, false);
  assert.equal(roomy.strings.length, 400, 'strings are capped at 400 and all fit here');
  assert.equal(roomy.truncated, true);

  const tight = buildJevState({ name: 'n' }, d, { maxChars: 6_000 });
  assert.ok(JSON.stringify(tight).length <= 6_000);
  assert.equal(tight.scripts.length, 2, 'both scripts keep a floor of the budget');
  assert.ok(tight.strings.length > 0 && tight.strings.length < 400, 'strings are what gets trimmed');
});

test('jevEvaluate normalizes answers and retries on 429', async () => {
  let n = 0;
  const fetchImpl = async () => { n += 1; return n === 1 ? new Response('slow down', { status: 429 }) : new Response(JSON.stringify(jevAnswers({ hate_incitement: 0.4 })), { status: 200 }); };
  const out = await jevEvaluate({ state: {}, questions: buildJevQuestions(), env: { JEV_API_KEY: 'k' }, fetchImpl });
  assert.equal(n, 2);
  assert.equal(out.answers.hate_incitement.p, 0.4);
  assert.equal(out.answers.content_rating.choice, 'all_ages');
  assert.equal(out.transport, 'typesafe');
  await assert.rejects(() => jevEvaluate({ state: {}, questions: {}, env: {}, fetchImpl }), /not configured/);
});

// ── decision policy ─────────────────────────────────────────────────────────

const sig = over => ({ ...deterministicSignals({ r2_key: 'x', description: STATEMENT_TEXT, rating_questionnaire: questionnaireRecord(questionnaireAnswers()) }, dossier(), { capturesCount: 2 }), ...over });
const triage = (over, s) => decideTriage({ signals: sig(s), jev: { answers: jevAnswers(over).answers && Object.fromEntries(Object.entries(jevAnswers(over).answers).map(([k, a]) => [k, a.type === 'noul' ? { type: 'noul', p: a.noul } : a])) }, thresholds: DEFAULT_THRESHOLDS });

test('hard categories quarantine at 0.85, hold at 0.35, CSAM holds from 0.20', () => {
  assert.equal(triage({ terrorism_or_extremist_promotion: 0.9 }).outcome, 'quarantine');
  assert.equal(triage({ terrorism_or_extremist_promotion: 0.5 }).outcome, 'hold');
  assert.equal(triage({ terrorism_or_extremist_promotion: 0.3 }).outcome, 'judge');
  assert.equal(triage({ csam_or_minor_sexualization: 0.25 }).outcome, 'hold');
  const t = triage({ csam_or_minor_sexualization: 0.25 });
  assert.equal(t.lane, 'legal');
  assert.equal(t.reasons[0].code, 'csam_or_minor_sexualization');
});

test('soft categories auto-reject only with real-world intent and no ambiguity', () => {
  assert.equal(triage({ real_crime_instructions: 0.9, real_world_intent: 0.8 }).outcome, 'reject');
  assert.equal(triage({ real_crime_instructions: 0.9, real_world_intent: 0.2 }).outcome, 'escalate');
  assert.equal(triage({ real_crime_instructions: 0.9, real_world_intent: 0.8, needs_human_context: 0.7 }).outcome, 'escalate');
  assert.equal(triage({ fraud_or_scam_facilitation: 0.5 }).outcome, 'escalate');
  assert.equal(triage({ fraud_or_scam_facilitation: 0.2 }).outcome, 'judge');
});

test('child-directed experiences with COPPA conflicts get changes requested, clean ones proceed', () => {
  const conflict = triage({ child_directed: 0.9, external_links_or_contact: 0.8 });
  assert.equal(conflict.outcome, 'changes_requested');
  assert.equal(conflict.lane, 'coppa');
  assert.ok(conflict.coppa_flags.some(f => f.code === 'external_links_or_contact'));
  const rated = triage({ child_directed: 0.9, content_rating: { choice: 'teen_13' } });
  assert.equal(rated.outcome, 'changes_requested');
  assert.ok(rated.coppa_flags.some(f => f.code === 'rating_above_all_ages'));
  const clean = triage({ child_directed: 0.9 });
  assert.equal(clean.outcome, 'judge');
  assert.equal(clean.child_directed, true);
  // Deterministic URL signal counts even when the classifier missed it.
  const det = triage({ child_directed: 0.9 }, { urls: 2 });
  assert.equal(det.outcome, 'changes_requested');
});

test('quality lane: empties are Code B8 rejects, confident asset flips are B7 holds, shaky ones escalate, solid ones judge', () => {
  assert.deepEqual([triage({}, { empty: true }).outcome, triage({}, { empty: true }).reasons[0].article], ['reject', 'B8']);
  assert.deepEqual([triage({}, { default_only: true }).reasons[0].code, triage({}, { default_only: true }).reasons[0].article], ['no_discernible_intent_default_only', 'B8']);
  const flip = level => ({ quality_band: { probabilities: Object.fromEntries(QUALITY_BANDS.map(b => [b, b === level ? 0.8 : 0.04])), confidence: 0.9 }, spatial_intent: 0.1 });
  const r = triage(flip('unmodified_template_or_asset_flip'));
  assert.equal(r.outcome, 'hold', 'a person decides B7'); assert.equal(r.lane, 'quality'); assert.equal(r.reasons[0].article, 'B7');
  assert.deepEqual([triage(flip('test_or_scratch_junk')).outcome, triage(flip('test_or_scratch_junk')).reasons[0].article], ['reject', 'B8']);
  for (const reason of [...triage({}, { empty: true }).reasons, ...r.reasons]) assert.equal('criterion' in reason, false, 'no v1.3 criterion numbers');
  const shaky = triage({ ...flip('unmodified_template_or_asset_flip'), quality_band: { ...flip('x').quality_band, probabilities: Object.fromEntries(QUALITY_BANDS.map(b => [b, b === 'unmodified_template_or_asset_flip' ? 0.5 : 0.1])), confidence: 0.4 } });
  assert.equal(shaky.outcome, 'escalate');
  const intent = triage({ ...flip('unmodified_template_or_asset_flip'), spatial_intent: 0.8 });
  assert.equal(intent.outcome, 'escalate', 'visible spatial intent blocks an automatic quality reject');
  const solid = triage({});
  assert.equal(solid.outcome, 'judge'); assert.equal(solid.quality_band, 'solid_authored_experience'); assert.equal(solid.rating, 'all_ages');
  assert.equal(triage({}, { rate_exceeded: true }).outcome, 'escalate');
});

test('the judge verdict finishes the case: a qualifying answer lists, flags hold, citations reject, failing tests reject, malformed holds', () => {
  const base = triage({});
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict())) }).outcome, 'approve');
  const proposed = decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ featured_candidate: true, scores: { build_quality: 3, lesson: 3, real_world: 3 } }))) });
  assert.deepEqual([proposed.outcome, proposed.featured, proposed.featured_candidate], ['approve', false, true], 'featured is proposed for a person, never set');
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ verdict: 'flag_for_human_review' }))) }).outcome, 'hold');
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ confidence: 0.3 }))) }).outcome, 'hold');
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(rejectVerdict())) }).outcome, 'reject');
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ verdict: 'reject' }))) }).outcome, 'hold', 'a reject that cites no Group A article is malformed');
  const low = decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ scores: { build_quality: 0 }, suggested_edit_to_publish: 'Arrange the parts.' }))) });
  assert.equal(low.outcome, 'reject'); assert.equal(low.lane, 'quality'); assert.equal(low.reasons.at(-1).test, 'build_quality');
  assert.equal(decideFinal({ triage: base, judge: { malformed: true, error: 'judge_unavailable_500' } }).outcome, 'hold');
  // v2.1: no agent takes the gray band. It is an A7 signal; a judge that does not cite it leaves the case to a person (rule 6).
  const esc = decideFinal({ triage: triage({ fraud_or_scam_facilitation: 0.5 }), judge: parseJudgeVerdict(JSON.stringify(verdict())) });
  assert.deepEqual([esc.outcome, esc.reasons.at(-1).code, esc.reasons.at(-1).article, esc.reasons.at(-1).rule], ['hold', 'article_signal_not_cited', 'A7', 6]);
});

test('parseJudgeVerdict refuses generic or missing spatial evidence and bad enums', () => {
  assert.equal(parseJudgeVerdict('```json\n' + JSON.stringify(verdict()) + '\n```').malformed, false);
  assert.equal(parseJudgeVerdict(JSON.stringify(verdict({ spatial_evidence: ['looks fine'] }))).error, 'judge_no_spatial_evidence');
  assert.equal(parseJudgeVerdict(JSON.stringify(verdict({ spatial_evidence: [] }))).error, 'judge_no_spatial_evidence');
  assert.equal(parseJudgeVerdict(JSON.stringify(verdict({ verdict: 'approve' }))).error, 'judge_bad_enum');
  assert.equal(parseJudgeVerdict('I think it is fine').error, 'judge_not_json');
  assert.equal(parseJudgeVerdict('').error, 'judge_empty');
});

// ── the pipeline against fake storage ───────────────────────────────────────

test('the judge prompt keeps one stable cacheable prefix across different publishes', async () => {
  const env = environment();
  const first = await publishedSim(env);
  const second = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-00000000000b', name: 'Other Town', pak: 'PAKBYTES-2' });
  const d = deps(env);
  await runModerationCase(first.id, env, d);
  await runModerationCase(second.id, env, d);
  assert.equal(d.calls.judge.length, 2);
  // The Claude judge: the system prompt is the cached prefix, the same bytes on every call; the case is in the user turn.
  const [a, b] = d.calls.judge;
  assert.equal(a.system.length, 1);
  assert.equal(a.system[0].text, b.system[0].text, 'the system prompt must not vary between publishes');
  assert.deepEqual(a.system[0].cache_control, { type: 'ephemeral' });
  assert.ok(a.system[0].text.length > 15000, 'the policy is the bulk of the prefix');
  assert.notEqual(a.messages[0].content.at(-1).text, b.messages[0].content.at(-1).text, 'the per-case block does vary');
});

test('a clean publish is approved, listed, and the judge saw the captures', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = deps(env);
  const rec = await runModerationCase(sim.id, env, d, { trigger: 'publish' });
  assert.equal(rec.status, 'approved');
  assert.equal(rec.decision.rating, 'all_ages');
  assert.equal(rec.judge.captures_used.length, 2);
  assert.equal(d.calls.judge.length, 1);
  // One user message: the captures as base64 image blocks, then the case. The policy is the system prompt, cached.
  const body = d.calls.judge[0];
  assert.equal(body.messages.length, 1);
  assert.equal(body.messages[0].role, 'user', 'no assistant prefill');
  const judgeInput = body.messages[0].content;
  const images = judgeInput.filter(i => i.type === 'image');
  assert.equal(images.length, 2);
  assert.deepEqual([images[0].source.type, images[0].source.media_type, typeof images[0].source.data], ['base64', 'image/png', 'string']);
  assert.match(body.system[0].text, /POLICY BEGIN/);
  assert.equal(judgeInput[0].type, 'image');
  assert.match(judgeInput.at(-1).text, /Case summary/);
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), true);
  assert.equal(after.moderation.status, 'approved');
  assert.equal(after.moderation.policy_version, '2.3');
  assert.equal(after.moderation.review_schema, 2);
  assert.equal(after.moderation.featured, false, 'featured is a person\'s call');
  // The judge read the statement the system extracted, and the strings with article terms first.
  const summary = JSON.parse(judgeInput.at(-1).text.split('\n')[2]);
  assert.deepEqual([summary.statement.present, summary.statement.category], [true, 'Simulation']);
  assert.ok(env.SOCIAL.m.has(`modq:approved:${sim.id}`));
  assert.ok(env.SOCIAL.m.has(`modroot:user-1:sha256:${createHash('sha256').update('PAKBYTES-1').digest('hex')}:${sim.scene_size_bytes}`), 'the verdict is remembered by the package\'s SHA-256');
  const pub = publicModeration(rec);
  assert.equal(pub.status, 'listed');
  assert.equal(pub.rating, 'all_ages');
  assert.ok(!('jev' in pub) && !('judge' in pub));
});

test('a hard category quarantines: nobody downloads, the author\'s account is not frozen, a person must release', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const audits = [];
  const d = { ...deps(env, { audits }), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.93 })) };
  const rec = await runModerationCase(sim.id, env, d, { trigger: 'publish' });
  assert.equal(rec.status, 'quarantined');
  assert.equal(rec.legal_hold.category, 'csam_or_minor_sexualization');
  assert.ok(rec.legal_hold.preserve_until > new Date(Date.now() + 360 * 86400 * 1000).toISOString());
  assert.equal(d.calls.judge.length, 0, 'the judge never runs on a quarantine');
  // McKale, 2026-10-15: the content is removed, the account is not frozen.
  assert.equal(env.USERS.m.has('publish-frozen:user-1'), false, 'no freeze is written');
  assert.ok(audits.some(a => a.action === 'moderation_quarantine'));
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), false);
  assert.equal(canServe(after, 'user-1', false), false, 'the author cannot pull a quarantined .pak');
  assert.equal(canServe(after, 'someone', false), false);
  assert.equal(canServe(after, 'admin', true), true);
  // The author is never told about the hold: it reads as ordinary review.
  const masked = publicModeration(rec, after);
  assert.equal(masked.status, 'in_review');
  assert.deepEqual(masked.reasons, []);
  assert.equal(masked.can_appeal, false);
  assert.doesNotMatch(JSON.stringify(masked), /quarantin|legal|csam/i);

  const exec = makeToolExecutor(env, d, { actor: 'agent', actorId: 'grok-agent' });
  const refused = await exec('moderation_approve', { sim_id: sim.id, rating: 'all_ages', rationale: 'The agent thinks this is fine actually.' });
  assert.equal(refused.ok, false);
  assert.match(refused.error, /release/);
  assert.equal((await exec('moderation_release', { sim_id: sim.id, rationale: 'agent trying to release this case' })).ok, false);

  const human = makeToolExecutor(env, d, { actor: 'admin', actorId: 'admin-7' });
  const released = await human('moderation_release', { sim_id: sim.id, rationale: 'Reviewed: a mislabelled anatomy lesson for adults, no minors involved.', rating: 'mature_17' });
  assert.equal(released.ok, true);
  const final = await loadCase(env, sim.id);
  assert.equal(final.status, 'approved');
  assert.equal(final.legal_hold, null);
  assert.equal(final.legal_release.released_by, 'admin-7');
  assert.equal(env.USERS.m.has('publish-frozen:user-1'), false);
  assert.equal(isListable(await simOf(env, sim.id)), true);
});

test('a middling hard probability holds for a person and the agent cannot approve it', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held');
  assert.equal(rec.decision.lane, 'legal');
  const exec = makeToolExecutor(env, d, { actor: 'agent' });
  const r = await exec('moderation_approve', { sim_id: sim.id, rating: 'teen_13', rationale: 'Names in the dialogue are fictional characters.' });
  assert.equal(r.ok, false);
  assert.match(r.error, /human must decide/);
});

test('a real-world crime manual is rejected with a suggested edit and stays reachable for its author', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'rejected');
  assert.equal(rec.decision.lane, 'harm');
  assert.match(rec.decision.suggested_edit, /real-world real crime instructions/);
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), false);
  assert.equal(canServe(after, 'user-1', false), true);
  assert.equal(canServe(after, 'other', false), false);
});

test('an empty Universe is rejected on the digest alone, even when Jev is down', async () => {
  const env = environment();
  const sim = await publishedSim(env, { withDossier: dossier({ digest: { entity_count: 0 } }) });
  const d = { ...deps(env), fetch: async () => new Response('boom', { status: 500 }) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'rejected');
  assert.deepEqual([rec.decision.reasons[0].code, rec.decision.reasons[0].article], ['non_functional_or_empty', 'B8']);
  assert.equal(rec.jev.error.includes('500'), true);
});

test('classifier or judge outages fail closed into the human queue, never into the gallery', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: async () => new Response('down', { status: 503 }) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held');
  assert.equal(rec.decision.reasons[0].code, 'classifier_unavailable');
  assert.equal(isListable(await simOf(env, sim.id)), false);

  const sim2 = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-000000000002' });
  const d2 = { ...deps(env), grokFetch: async () => ({ ok: false, status: 529, text: async () => 'overloaded' }) };
  const rec2 = await runModerationCase(sim2.id, env, d2);
  assert.equal(rec2.status, 'held');
  assert.equal(rec2.judge.error, 'judge_unavailable_529');

  // No ANTHROPIC_API_KEY and no client: the judge is unavailable, and the case is held (fail closed).
  const env3 = environment();
  const sim3 = await publishedSim(env3);
  const rec3 = await runModerationCase(sim3.id, env3, { ...deps(env3), claude: undefined });
  assert.equal(rec3.status, 'held');
  assert.equal(rec3.judge.error, 'judge_unavailable_no_key');
});

test('a republish of an already decided .pak by the same author copies the decision without running the models', async () => {
  const env = environment();
  const first = await publishedSim(env);
  const calls = [];
  const d = { ...deps(env), fetch: jevFetch(jevAnswers(), calls) };
  await runModerationCase(first.id, env, d);
  assert.equal(calls.length, 1);
  const second = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-000000000009', pak: 'PAKBYTES-1' });
  const rec = await runModerationCase(second.id, env, d);
  assert.equal(rec.status, 'approved');
  assert.equal(rec.decision.dedup_of, first.id);
  assert.equal(calls.length, 1, 'no second Jev call');
  assert.equal(d.calls.judge.length, 1, 'no second judge call');
  // A different author with the same bytes is judged on their own.
  const third = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-000000000010', author: 'user-2', pak: 'PAKBYTES-1' });
  await runModerationCase(third.id, env, d);
  assert.equal(calls.length, 2);
});

// Code v2.1 retired the Grok agent. These three replace its tests: the gray band it took, the audit it re-read, and the
// escalation it could leave undecided now all go to a person in the Review Workspace, whose members are told.
const WORKSPACE = { space_id: 'b0b0b0b0-0000-4000-8000-00000000beef', members: ['admin-7', 'admin-8'] };

test('the gray band is decided by the rules, never by a model: a judge that cites nothing leaves it to the Review Workspace, whose members are told', async () => {
  const env = environment();
  await env.SOCIAL.put('workspace:review', JSON.stringify(WORKSPACE));
  const sim = await publishedSim(env);
  const xai = [];
  const grok = async body => { if (body.tools) xai.push(body); return grokJudge(verdict())(body); };
  const d = { ...deps(env, { grok }), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.55 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held', 'the judge would publish; the rules hold an uncited Group A signal for a person');
  assert.deepEqual([rec.decision.reasons.at(-1).code, rec.decision.reasons.at(-1).article, rec.decision.reasons.at(-1).rule], ['article_signal_not_cited', 'A4', 6]);
  assert.equal('agent' in rec && rec.agent !== null, false, 'no agent ran');
  assert.equal(xai.filter(b => !b.tools?.some(t => t.type === 'web_search')).length, 0, 'no model was handed a tool to decide with');
  const queued = JSON.parse(env.SOCIAL.m.get(`wsq:hold:${sim.id}`));
  assert.deepEqual([queued.kind, queued.reference, queued.review_schema], ['hold', rec.reference, 2]);
  for (const member of WORKSPACE.members) {
    const feed = [...env.SOCIAL.m.keys()].filter(k => k.startsWith(`notif:${member}:`)).map(k => JSON.parse(env.SOCIAL.m.get(k)));
    assert.deepEqual(feed.map(i => [i.kind, i.event, i.sim_id]), [['review_workspace', 'hold', sim.id]], member);
  }
  const admin = makeToolExecutor(env, d, { actor: 'admin', actorId: 'admin-7' });
  const q = await admin('moderation_workspace_queue', {});
  assert.deepEqual(q.items.map(i => [i.kind, i.sim_id, i.status]), [['hold', sim.id, 'held']]);
  // A person decides; the case leaves the queue.
  assert.equal((await admin('moderation_reject', { sim_id: sim.id, lane: 'harm', category: 'A4', rationale: 'The lab script is a usable synthesis route, not a lesson.' })).ok, true);
  assert.equal(env.SOCIAL.m.has(`wsq:hold:${sim.id}`), false);
  assert.deepEqual((await admin('moderation_workspace_queue', {})).items, []);
});

test('an audit sample of approvals and of rejections goes to the Review Workspace for a person, who closes it', async () => {
  const env = environment({ MODERATION_THRESHOLDS: '{"audit_rate":1}' });
  await env.SOCIAL.put('workspace:review', JSON.stringify(WORKSPACE));
  const sim = await publishedSim(env);
  const d = deps(env);
  const rec = await runModerationCase(sim.id, env, d);
  assert.deepEqual([rec.status, rec.decision.audit_sampled, rec.audit.status, rec.audit.kind], ['approved', true, 'pending', 'approval']);
  assert.equal(isListable(await simOf(env, sim.id)), true, 'an audit does not take the listing down');
  assert.ok(env.SOCIAL.m.has(`wsq:audit:${sim.id}`));
  const sim2 = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-0000000000a2', pak: 'PAKBYTES-A2' });
  const rejected = await runModerationCase(sim2.id, env, { ...deps(env, { judge: rejectVerdict() }) });
  assert.deepEqual([rejected.status, rejected.audit.kind], ['rejected', 'rejection'], 'Code Part VIII: a person audits rejections');
  const admin = makeToolExecutor(env, d, { actor: 'admin', actorId: 'admin-7' });
  assert.deepEqual((await admin('moderation_workspace_queue', { kind: 'audit' })).items.map(i => i.sim_id).sort(), [sim.id, sim2.id].sort());
  assert.equal((await makeToolExecutor(env, d, { actor: 'agent' })('moderation_audit_resolve', { sim_id: sim.id, rationale: 'An agent may not close an audit.' })).ok, false);
  assert.equal((await admin('moderation_audit_resolve', { sim_id: sim.id, rationale: 'Read the captures and the scripts; the approval stands.' })).ok, true);
  assert.equal((await loadCase(env, sim.id)).audit.status, 'done');
  assert.deepEqual((await admin('moderation_workspace_queue', { kind: 'audit' })).items.map(i => i.sim_id), [sim2.id]);
});

test('an escalated case calls no model past the judge: the gray band is held with the article named, and the researcher is not asked when nothing public is at stake', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const xai = [];
  const grok = async body => { xai.push(body); return grokJudge(verdict())(body); };
  const d = { ...deps(env, { grok }), fetch: jevFetch(jevAnswers({ hate_incitement: 0.5 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held');
  assert.ok(rec.decision.reasons.some(r => r.code === 'article_signal_not_cited' && r.article === 'A9'));
  assert.equal(xai.length, 1, 'one call: the judge');
  assert.equal(rec.research, null);
});

test('the agent cannot approve without a valid judge verdict, cannot rerun, and cannot lift a hold', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), grokFetch: async () => ({ ok: false, status: 500, text: async () => '' }) };
  await runModerationCase(sim.id, env, d);
  const exec = makeToolExecutor(env, d, { actor: 'agent' });
  const approve = await exec('moderation_approve', { sim_id: sim.id, rating: 'all_ages', rationale: 'The digest shows a well-composed harbor scene.' });
  assert.equal(approve.ok, false); assert.match(approve.error, /judge verdict/);
  assert.equal((await exec('moderation_rerun', { sim_id: sim.id })).ok, false);
  assert.equal((await exec('moderation_backfill', {})).ok, false);
  assert.equal((await exec('moderation_resolve_appeal', { sim_id: sim.id, decision: 'overturned', rationale: 'x'.repeat(30) })).ok, false);
  const hold = await exec('moderation_hold', { sim_id: sim.id, reason: 'Judge unavailable; needs eyes.' });
  assert.equal(hold.ok, true);
  const short = await exec('moderation_reject', { sim_id: sim.id, lane: 'quality', category: 'asset_flip', rationale: 'meh' });
  assert.equal(short.ok, false); assert.match(short.error, /20 characters/);
});

// ── routes ──────────────────────────────────────────────────────────────────

const req = (method, path, { token, body, headers = {} } = {}) => new Request(`https://api.eustress.dev${path}`, { method, headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...headers }, ...(body === undefined ? {} : { body }) });
const route = (request, env, d, ctx) => handleModerationRoute(request, new URL(request.url), env, ctx, d);

test('dossier and capture uploads validate size, shape and image magic', async () => {
  const env = environment();
  const sim = await publishedSim(env, { withDossier: null, withCaptures: 0 });
  const d = deps(env);
  let r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-1', body: JSON.stringify({ dossier_version: 7 }) }), env, d);
  assert.equal(r.status, 400);
  r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-1', body: 'x'.repeat(DOSSIER_MAX_BYTES + 1) }), env, d);
  assert.equal(r.status, 413);
  r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-2', body: JSON.stringify(dossier()) }), env, d);
  assert.equal(r.status, 403);
  r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-1', body: JSON.stringify(dossier()) }), env, d);
  assert.equal(r.status, 200);
  assert.equal((await simOf(env, sim.id)).content_root, 'blake3:abc');
  r = await route(req('PUT', `/api/simulations/${sim.id}/captures/0`, { token: 'user-1', body: new Uint8Array([1, 2, 3, 4, 5]) }), env, d);
  assert.equal(r.status, 415);
  r = await route(req('PUT', `/api/simulations/${sim.id}/captures/0`, { token: 'user-1', body: validPng(0) }), env, d);
  assert.equal(r.status, 200);
  assert.ok(env.SCENES.m.has(`universes/${sim.id}/moderation/capture-0.png`));
  assert.equal(await route(req('PUT', `/api/simulations/${sim.id}/captures/9`, { token: 'user-1', body: 'x' }), env, d), null, 'index out of range is not a moderation route');
});

test('submit runs the pipeline, answers with the decision, and the author can read the outcome and appeal', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.9, real_world_intent: 0.9 })) };
  const pending = [];
  const ctx = { waitUntil: p => pending.push(p) };
  const r = await route(req('POST', `/api/simulations/${sim.id}/submit`, { token: 'user-1' }), env, d, ctx);
  // The response is held until the pipeline has decided (see the submit route).
  assert.equal(r.status, 200);
  assert.equal((await r.json()).status, 'not_listed');
  await Promise.all(pending);
  const status = await route(req('GET', `/api/simulations/${sim.id}/moderation`, { token: 'user-1' }), env, d);
  const body = await status.json();
  assert.equal(body.status, 'not_listed');
  assert.equal(body.can_appeal, true);
  assert.equal(body.reasons[0].category, 'harm_real_world_instructions');
  assert.equal(body.listable, false);
  assert.ok(body.suggested_edit);
  assert.equal(await (await route(req('GET', `/api/simulations/${sim.id}/moderation`, { token: 'stranger' }), env, d)).status, 403);
  const bad = await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'short' }) }), env, d);
  assert.equal(bad.status, 400);
  const ok = await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'The chemistry strings are flavor text in a fictional lab; nothing is a real procedure.' }) }), env, d, ctx);
  assert.equal(ok.status, 202);
  await Promise.all(pending);
  const rec = await loadCase(env, sim.id);
  assert.equal(rec.status, 'appealed');
  assert.equal(rec.appeal.status, 'pending');
  const again = await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'Second appeal while one is pending should be refused.' }) }), env, d, ctx);
  assert.equal(again.status, 409);
  const admin = { ...d, requireAdmin: async () => 'admin-1' };
  const resolved = await route(req('POST', '/api/admin/moderation/tool', { token: 'admin-1', body: JSON.stringify({ name: 'moderation_resolve_appeal', args: { sim_id: sim.id, decision: 'overturned', rationale: 'Read the scripts: constants named after reagents, no quantities, no steps.', rating: 'teen_13' } }) }), env, admin, ctx);
  assert.equal(resolved.status, 200);
  assert.equal(isListable(await simOf(env, sim.id)), true);
});

test('admin routes require admin and expose the queue, the case, the captures and the tool catalog', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env, { admin: 'admin-1' }), fetch: jevFetch(jevAnswers({ ncii_or_real_person_sexual: 0.5 })) };
  await runModerationCase(sim.id, env, d);
  assert.equal((await route(req('GET', '/api/admin/moderation/queue', { token: 'user-1' }), env, d)).status, 403);
  const q = await (await route(req('GET', '/api/admin/moderation/queue?status=held', { token: 'admin-1' }), env, d)).json();
  assert.equal(q.items.length, 1);
  assert.equal(q.items[0].sim_id, sim.id);
  const c = await (await route(req('GET', `/api/admin/moderation/case/${sim.id}`, { token: 'admin-1' }), env, d)).json();
  assert.equal(c.case.jev.answers.ncii_or_real_person_sexual.p, 0.5);
  const cap = await route(req('GET', `/api/admin/moderation/captures/${sim.id}/0`, { token: 'admin-1' }), env, d);
  assert.equal(cap.status, 200);
  assert.equal(cap.headers.get('Content-Type'), 'image/png');
  const tools = await (await route(req('GET', '/api/admin/moderation/tools', { token: 'admin-1' }), env, d)).json();
  assert.equal(tools.tools.length, MODERATION_TOOLS.length);
  assert.equal(tools.policy_hash, POLICY_HASH_ANCHORED);
  assert.equal(tools.thresholds.hard_hold, 0.35);
});

test('thresholds can be overridden from the environment without touching code', async () => {
  const env = environment({ MODERATION_THRESHOLDS: '{"hard_hold":0.9,"bogus":1,"audit_rate":"no"}' });
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'approved', 'with hard_hold raised to 0.9 a 0.5 no longer holds');
});

test('a private publish is still screened for the legal lane but never listed', async () => {
  const env = environment();
  const sim = await publishedSim(env, { is_public: false });
  const rec = await runModerationCase(sim.id, env, deps(env));
  assert.equal(rec.status, 'approved');
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), false);
  assert.equal(canServe(after, 'user-1', false), true);
  assert.equal(canServe(after, 'other', false), false);
});

// ── the author's view, the feed and the email ──────────────────────────────

// An inbox and a sender the Worker's EMAIL binding stands in for.
function mailEnv(extra = {}) {
  const env = environment({ JWT_SECRET: 'test-secret', ...extra });
  env.sent = [];
  env.EMAIL = { send: async m => { env.sent.push(m); } };
  env.__EmailMessage = class { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } };
  env.USERS.m.set('user:user-1', JSON.stringify({ id: 'user-1', username: 'ann', email: 'ann@example.com' }));
  return env;
}
const feedOf = env => [...env.SOCIAL.m.keys()].filter(k => k.startsWith('notif:user-1:')).map(k => JSON.parse(env.SOCIAL.m.get(k)));
const bodyOf = m => Buffer.from(m.raw.split('Content-Transfer-Encoding: base64\r\n\r\n')[1].split('\r\n--')[0].replace(/\r\n/g, ''), 'base64').toString('utf8');
const notifRoute = (request, env) => handleNotificationRoute(request, new URL(request.url), env, { verifyAuth: async r => (r.headers.get('Authorization') || '').replace('Bearer ', '') || null, json, cors: {} });

const INTERNAL_KEYS = ['p', 'confidence', 'threshold', 'rationale', 'spatial_evidence', 'jev', 'judge', 'by', 'legal_hold', 'lane', 'code', 'triage', 'agent', 'history', 'human'];
function keysDeep(v, out = new Set()) {
  if (Array.isArray(v)) v.forEach(x => keysDeep(x, out));
  else if (v && typeof v === 'object') for (const [k, x] of Object.entries(v)) { out.add(k); keysDeep(x, out); }
  return out;
}

test('the author view never carries internal fields, for every internal status', () => {
  const internals = {
    reasons: [{ code: 'quality_unmodified_template_or_asset_flip', article: 'B7', lane: 'quality', p: 0.93, confidence: 0.91, action: 'reject' },
              { code: 'judge_flagged', lane: 'judge', action: 'hold', rationale: 'judge thinks p=0.44' },
              { code: 'classifier_unavailable', lane: 'system', action: 'hold' }],
    suggested_edit: 'Arrange the arcade facades yourself.', rating: 'teen_13', lane: 'quality', changes: ['external_links_or_contact'],
  };
  for (const status of ['pending', 'classifying', 'approved', 'held', 'rejected', 'changes_requested', 'quarantined', 'appealed']) {
    const rec = { sim_id: 's', author_id: 'a', status, decision: internals, jev: { answers: { csam_or_minor_sexualization: { p: 0.9 } } }, judge: { rationale: 'x', spatial_evidence: ['y'] },
      author_notice: { message: 'Remove the Discord link from the lobby sign.', by: 'grok-agent', changes: ['external_links_or_contact'] },
      legal_hold: status === 'quarantined' ? { category: 'csam_or_minor_sexualization' } : null,
      appeal: status === 'appealed' ? { status: 'pending', at: 't', text: 'please', rationale: 'internal' } : null, updated_at: 'u', policy_version: '1.2' };
    const v = authorView(rec, { is_public: true });
    const keys = keysDeep(v);
    for (const k of INTERNAL_KEYS) assert.ok(!keys.has(k), `${status}: leaked key ${k}`);
    const text = JSON.stringify(v);
    assert.doesNotMatch(text, /0\.9\d|quarantin|legal|csam|judge|classifier|grok|held_by|under_legal_review/i, `${status}: ${text}`);
    assert.ok(['in_review', 'listed', 'not_listed', 'changes_requested', 'appeal_in_review'].includes(v.status), status);
  }
  assert.equal(authorView({ status: 'held' }, {}).status, authorView({ status: 'quarantined' }, {}).status, 'held and quarantined are indistinguishable');
  assert.equal(authorView({ status: 'held' }, {}).headline, authorView({ status: 'quarantined' }, {}).headline);
  assert.equal(authorStatusOf('approved', false), 'approved_private');
});

test('reasons come from the catalogue, and unknown codes fall back to the lane copy', () => {
  const view = authorView({ status: 'rejected', decision: { reasons: [
    { code: 'brand_new_internal_code_v9', lane: 'quality', action: 'reject' },
    { code: 'another_unknown', lane: 'harm', action: 'reject' },
    { code: 'real_crime_instructions', lane: 'harm', action: 'reject' },
    { code: 'ignored_escalation', lane: 'harm', action: 'escalate' },
  ] } }, {});
  assert.deepEqual(view.reasons.map(r => r.category), ['code_not_met', 'harm_policy', 'harm_real_world_instructions']);
  for (const r of view.reasons) { assert.ok(r.title && r.why && r.what_to_change); assert.equal(r.title, AUTHOR_COPY[r.category].title); }
  const coppa = authorView({ status: 'changes_requested', decision: { changes: ['external_links_or_contact', 'collects_personal_info', 'Rename the "Kill" button to "Tag"'] } }, {});
  assert.deepEqual(coppa.reasons.map(r => r.category), ['coppa_links', 'coppa_personal_data', 'requested_change']);
  assert.equal(coppa.reasons[2].what_to_change, 'Rename the "Kill" button to "Tag"');
});

test('free text shown to an author drops anything about how the review works', () => {
  assert.equal(safeAuthorText('Add original lighting to the plaza.'), 'Add original lighting to the plaza.');
  assert.equal(safeAuthorText('Score = 0.91 on the flip scale; add lighting.'), 'on the flip scale; add lighting.');
  assert.equal(safeAuthorText('Per policy v1.2 section 5 this fails.'), null);
  assert.equal(safeAuthorText('The classifier was confident, p=0.93.'), null);
  assert.equal(safeAuthorText('Below the threshold for quality.'), null);
  assert.equal(safeAuthorText('A two-lane road and a model train set are fine.'), 'A two-lane road and a model train set are fine.');
  assert.equal(safeAuthorText('x'.repeat(900)).length, 600);
  assert.equal(safeAuthorText(42), null);
});

test('a decision reaches the author exactly once, in the feed and by email', async () => {
  const env = mailEnv();
  const sim = await publishedSim(env);
  const d = deps(env);
  await runModerationCase(sim.id, env, d);
  const feed = feedOf(env);
  assert.equal(feed.length, 1);
  assert.deepEqual([feed[0].status, feed[0].event, feed[0].read], ['listed', 'decision', false]);
  assert.equal(env.sent.length, 1);
  assert.equal(env.sent[0].to, 'ann@example.com');
  assert.match(env.sent[0].raw, /^Subject: Your Universe "Harbor Town" is live in the Gallery\r$/m);
  assert.match(env.sent[0].raw, /^List-Unsubscribe: <https:\/\/api\.eustress\.dev\/api\/notifications\/unsubscribe\?u=user-1&t=[0-9a-f]{64}>\r$/m);
  assert.match(env.sent[0].raw, /^List-Unsubscribe-Post: List-Unsubscribe=One-Click\r$/m);
  const text = bodyOf(env.sent[0]);
  assert.match(text, /Age rating: All ages/);
  assert.doesNotMatch(text, /0\.\d\d|judge|jev|grok|polic|threshold|spatial/i);
  // A rerun and the sweep reach the same decision: nothing new is sent.
  await runModerationCase(sim.id, env, d, { trigger: 'rerun' });
  await runModerationCase(sim.id, env, d, { trigger: 'sweep' });
  assert.equal(feedOf(env).length, 1);
  assert.equal(env.sent.length, 1);
});

test('a decision email goes to the address the account\'s object keeps, not the one the user record was made with', async () => {
  const env = mailEnv();
  env.USERS.m.set('email:user-1', JSON.stringify({ address: 'ann-new@example.com', verified_at: null }));
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, deps(env));
  assert.equal(env.sent.length, 1);
  assert.equal(env.sent[0].to, 'ann-new@example.com');
});

test('held and quarantined cases tell the author nothing, by feed or by email', async () => {
  const env = mailEnv();
  const held = await publishedSim(env);
  await runModerationCase(held.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) });
  const q = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-0000000000f1', pak: 'PAK-Q' });
  await runModerationCase(q.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) });
  assert.equal((await loadCase(env, q.id)).status, 'quarantined');
  assert.equal(feedOf(env).length, 0);
  // Staff are told the CSAM report waits for its sender (src/legal_reports.mjs); the author is told nothing.
  assert.equal(env.sent.filter(m => m.to === 'ann@example.com').length, 0);
  assert.ok(env.sent.some(m => m.to === 'security@eustress.dev'), 'the staff alert went to the staff mailbox');
  const r = await route(req('GET', `/api/simulations/${q.id}/moderation`, { token: 'user-1' }), env, deps(env));
  const body = await r.json();
  assert.equal(body.status, 'in_review');
  assert.doesNotMatch(JSON.stringify(body), /quarantin|legal/i);
  const appeal = await route(req('POST', `/api/simulations/${q.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'Why is this still in review after a day?' }) }), env, deps(env));
  assert.equal(appeal.status, 409);
  const ab = await appeal.json();
  assert.equal(ab.status, 'in_review');
  assert.doesNotMatch(JSON.stringify(ab), /quarantin/i);
});

test('an opted-out author still gets the feed item but no email', async () => {
  const env = mailEnv();
  env.USERS.m.set('notify-prefs:user-1', JSON.stringify({ moderation_email: false }));
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) });
  assert.equal(feedOf(env)[0].status, 'not_listed');
  assert.equal(feedOf(env)[0].can_appeal, true);
  assert.equal(env.sent.length, 0);
});

test('a new publish of the same listing is announced again; an appeal decision is announced once', async () => {
  const env = mailEnv();
  const sim = await publishedSim(env);
  const reject = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) };
  await runModerationCase(sim.id, env, reject);
  assert.match(env.sent[0].raw, /^Subject: "Harbor Town" was not listed in the Gallery\r$/m);
  await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'The chemistry is fictional flavour text with no steps.' }) }), env, { ...deps(env), playbookText: '' });
  const admin = makeToolExecutor(env, deps(env), { actor: 'admin', actorId: 'admin-1' });
  const r = await admin('moderation_resolve_appeal', { sim_id: sim.id, decision: 'overturned', rationale: 'Constants named after reagents; no quantities or steps anywhere.', rating: 'teen_13' });
  assert.equal(r.ok, true);
  assert.equal(env.sent.length, 2);
  assert.match(env.sent[1].raw, /^Subject: Appeal accepted: "Harbor Town" is now listed\r$/m);
  const appealItems = feedOf(env).filter(i => i.event === 'appeal_decided');
  assert.equal(appealItems.length, 1);
  assert.equal(appealItems[0].status, 'listed');
  // A republish is a new decision, and the pipeline says so again.
  await runModerationCase(sim.id, env, reject, { trigger: 'publish' });
  assert.equal(env.sent.length, 3);
});

test('a listing name cannot inject mail headers', async () => {
  const env = mailEnv();
  const sim = await publishedSim(env, { name: 'Evil\r\nBcc: victim@example.com\r\nX' });
  await runModerationCase(sim.id, env, deps(env));
  const raw = env.sent[0].raw;
  assert.doesNotMatch(raw, /^Bcc:/m);
  assert.equal(raw.split('\r\n\r\n')[0].split('\r\n').filter(l => l.startsWith('Subject:')).length, 1);
});

test('unsubscribe: GET only shows a button, POST with a valid token opts out, a forged token is refused', async () => {
  const env = mailEnv();
  const t = await unsubscribeToken(env, 'user-1');
  const url = `/api/notifications/unsubscribe?u=user-1&t=${t}`;
  const get = await notifRoute(req('GET', url), env);
  assert.equal(get.status, 200);
  assert.match(await get.text(), /<form method="post"/);
  assert.equal(env.USERS.m.get('notify-prefs:user-1'), undefined, 'a GET (mail scanner) must not unsubscribe');
  const post = await notifRoute(req('POST', url, { body: 'List-Unsubscribe=One-Click', headers: { 'Content-Type': 'application/x-www-form-urlencoded' } }), env);
  assert.equal(post.status, 200);
  assert.equal(JSON.parse(env.USERS.m.get('notify-prefs:user-1')).moderation_email, false);
  const forged = await notifRoute(req('POST', `/api/notifications/unsubscribe?u=user-2&t=${t}`), env);
  assert.equal(forged.status, 400);
  assert.equal(env.USERS.m.get('notify-prefs:user-2'), undefined);
  assert.equal((await notifRoute(req('POST', '/api/notifications/unsubscribe?u=user-1&t=00'), env)).status, 400);
});

test('the feed is per account, honours since, marks read, and is rate limited', async () => {
  const env = mailEnv();
  const put = (user, at, status) => env.SOCIAL.m.set(`notif:${user}:${at}:s`, JSON.stringify({ id: `${at}:s`, status, at, read: false, expires_at: new Date(Date.now() + 86400e3).toISOString() }));
  put('user-1', '2026-09-26T01:00:00Z', 'not_listed');
  put('user-1', '2026-09-26T02:00:00Z', 'listed');
  put('user-2', '2026-09-26T03:00:00Z', 'listed');
  assert.equal((await notifRoute(req('GET', '/api/notifications'), env)).status, 401);
  let body = await (await notifRoute(req('GET', '/api/notifications', { token: 'user-1' }), env)).json();
  assert.deepEqual(body.items.map(i => i.status), ['listed', 'not_listed']);
  assert.equal(body.unread, 2);
  body = await (await notifRoute(req('GET', '/api/notifications?since=2026-09-26T01:30:00Z', { token: 'user-1' }), env)).json();
  assert.deepEqual(body.items.map(i => i.status), ['listed']);
  const read = await notifRoute(req('POST', '/api/notifications/read', { token: 'user-1', body: JSON.stringify({ ids: ['2026-09-26T02:00:00Z:s', '2026-09-26T03:00:00Z:s'] }) }), env);
  assert.equal((await read.json()).marked, 1, 'another account cannot mark items it does not own');
  assert.equal(JSON.parse(env.SOCIAL.m.get('notif:user-2:2026-09-26T03:00:00Z:s')).read, false);
  let calls = 0;
  env.NOTIFY_RATE_LIMITER = { limit: async ({ key }) => { calls += 1; assert.equal(key, 'notif:user-1'); return { success: calls <= 1 }; } };
  assert.equal((await notifRoute(req('GET', '/api/notifications', { token: 'user-1' }), env)).status, 200);
  const limited = await notifRoute(req('GET', '/api/notifications', { token: 'user-1' }), env);
  assert.equal(limited.status, 429);
  assert.equal(limited.headers.get('Retry-After'), '60');
  const prefs = await notifRoute(req('PUT', '/api/notifications/prefs', { token: 'user-9', body: JSON.stringify({ moderation_email: true }) }), { ...env, NOTIFY_RATE_LIMITER: undefined });
  assert.equal((await prefs.json()).moderation_email, true);
});

// ── listings stranded at pending ────────────────────────────────────────────

const HOUR = 3600 * 1000;
const iso = ago => new Date(Date.now() - ago).toISOString();
const SIM = n => `a1b2c3d4-0000-4000-8000-0000000001${String(n).padStart(2, '0')}`;
// A listing whose publish stored the content and then never reached review.
async function strandedSim(env, n, { ageMs = HOUR, author = 'user-1', withDossier = true, status = 'pending' } = {}) {
  const sim = await publishedSim(env, { id: SIM(n), author, pak: `PAK-STRANDED-${n}`, withDossier: withDossier ? dossier() : null });
  sim.moderation = { status, submitted_at: iso(ageMs), ...(status === 'pending' ? {} : { updated_at: iso(ageMs) }) };
  sim.updated_at = iso(ageMs);
  sim.published_at = iso(ageMs);
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(sim));
  return sim;
}

test('a listing stranded at pending is resumed by the sweep and its author is told', async () => {
  const env = mailEnv();
  const sim = await strandedSim(env, 1);
  assert.equal(await loadCase(env, sim.id), null, 'no case exists, so the classifying resume cannot see it');
  const d = deps(env);
  const out = await sweepModeration(env, d, { backfill: false });
  assert.equal(out.stalled, 1);
  const rec = await loadCase(env, sim.id);
  assert.equal(rec.status, 'approved');
  assert.equal(rec.trigger, 'sweep');
  const after = await simOf(env, sim.id);
  assert.equal(after.moderation.status, 'approved');
  assert.equal(after.moderation.sweep_attempts, 0, 'a decision ends the retries');
  assert.equal(feedOf(env).length, 1);
  assert.equal(env.sent.length, 1);
  // Nothing is pending any more: the next tick does no work.
  assert.equal((await sweepModeration(env, d, { backfill: false })).stalled, 0);
  assert.equal(d.calls.judge.length, 1);
});

test('a stranded listing with no dossier or captures is held for a person, not approved', async () => {
  const env = mailEnv();
  const sim = await strandedSim(env, 2, { withDossier: false });
  await env.SCENES.delete(`universes/${sim.id}/moderation/capture-0.png`);
  await env.SCENES.delete(`universes/${sim.id}/moderation/capture-1.png`);
  await sweepModeration(env, { ...deps(env), grokFetch: grokJudge(verdict({ verdict: 'flag_for_human_review', confidence: 0.3 })) }, { backfill: false });
  const rec = await loadCase(env, sim.id);
  assert.equal(rec.status, 'held');
  assert.equal(isListable(await simOf(env, sim.id)), false);
  assert.equal(feedOf(env).length, 0, 'held reads as in_review to the author, so nothing is announced');
});

test('the stalled sweep leaves alone what it must', async () => {
  const env = mailEnv();
  const d = deps(env);
  const fresh = await strandedSim(env, 3, { ageMs: 2 * 60 * 1000 });
  const noContent = await strandedSim(env, 4);
  const sim4 = await simOf(env, noContent.id); sim4.r2_key = null; await env.SOCIAL.put(`sim:${noContent.id}`, JSON.stringify(sim4));
  const missingObject = await strandedSim(env, 5);
  await env.SCENES.delete(`universes/${missingObject.id}/universe.pak`);
  const classifying = await strandedSim(env, 6, { status: 'classifying' });
  const exhausted = await strandedSim(env, 7);
  const sim7 = await simOf(env, exhausted.id); sim7.moderation.sweep_attempts = 3; await env.SOCIAL.put(`sim:${exhausted.id}`, JSON.stringify(sim7));
  const approved = await strandedSim(env, 8, { status: 'approved' });
  // A real quarantine whose listing record was knocked back to pending.
  const q = await strandedSim(env, 9);
  await runModerationCase(q.id, env, { ...d, fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) });
  assert.equal((await loadCase(env, q.id)).status, 'quarantined');
  const quar = await simOf(env, q.id);
  quar.moderation = { status: 'pending', updated_at: iso(HOUR) }; quar.updated_at = iso(HOUR);
  await env.SOCIAL.put(`sim:${q.id}`, JSON.stringify(quar));
  const jevCalls = [];
  const quiet = { ...d, fetch: jevFetch(jevAnswers(), jevCalls) };

  assert.equal((await sweepModeration(env, quiet, { backfill: false })).stalled, 0);
  assert.equal(jevCalls.length, 0, 'no model was called for any of them');
  for (const s of [fresh, noContent, missingObject, exhausted]) assert.equal((await simOf(env, s.id)).moderation.status, 'pending', s.id);
  assert.equal((await simOf(env, classifying.id)).moderation.status, 'classifying');
  assert.equal((await simOf(env, approved.id)).moderation.status, 'approved');
  assert.equal((await loadCase(env, q.id)).status, 'quarantined', 'a legal hold is never reopened by a sweep');
  assert.equal(env.USERS.m.has('publish-frozen:user-1'), false, 'and the account is not frozen');
});

test('the stalled sweep resumes at most the per-run limit and picks the rest up next tick', async () => {
  const env = mailEnv({ MODERATION_THRESHOLDS: '{"audit_rate":0,"stalled_pending_per_run":2}' });
  const d = deps(env);
  const ids = [];
  for (let n = 10; n < 15; n++) ids.push((await strandedSim(env, n)).id);
  const decided = async () => (await Promise.all(ids.map(id => loadCase(env, id)))).filter(r => r && r.status === 'approved').length;
  assert.equal(await resumeStalledPending(env, d, { ...DEFAULT_THRESHOLDS, stalled_pending_per_run: 2 }), 2);
  assert.equal(await decided(), 2);
  assert.equal(await resumeStalledPending(env, d, { ...DEFAULT_THRESHOLDS, stalled_pending_per_run: 2 }), 2);
  assert.equal(await resumeStalledPending(env, d, { ...DEFAULT_THRESHOLDS, stalled_pending_per_run: 2 }), 1);
  assert.equal(await decided(), 5);
  assert.equal(await resumeStalledPending(env, d, { ...DEFAULT_THRESHOLDS, stalled_pending_per_run: 2 }), 0);
});

test('a run that throws uses up an attempt, and a half-claimed case is picked up later', async () => {
  const env = mailEnv();
  const sim = await strandedSim(env, 20);
  const d = deps(env);
  // Storage fails while the run reads the dossier: it has already claimed the case.
  const realGet = env.SCENES.get;
  env.SCENES.get = async () => { throw new Error('r2 unavailable'); };
  assert.equal(await resumeStalledPending(env, d), 1, 'the throw is contained, not propagated');
  env.SCENES.get = realGet;
  const s = await simOf(env, sim.id);
  assert.equal(s.moderation.sweep_attempts, 1);
  // The run needs storage for its lease before it writes anything, so a storage
  // outage claims nothing: the listing stays pending and is tried again later.
  assert.equal(s.moderation.status, 'pending');
  assert.equal(await loadCase(env, sim.id), null, 'no case was written');

  // The listing says pending but a case says classifying, with no queue key
  // (the run died between its two writes): only this scan can find it.
  const half = await strandedSim(env, 21);
  env.SOCIAL.m.set(`modcase:${half.id}`, JSON.stringify({ sim_id: half.id, author_id: 'user-1', status: 'classifying', classifying_since: iso(HOUR), history: [] }));
  assert.equal(env.SOCIAL.m.has(`modq:classifying:${half.id}`), false);
  assert.equal(await resumeStalledPending(env, d), 1);
  assert.equal((await loadCase(env, half.id)).status, 'approved');
});

test('the sweep scan is bounded and rotates through the listings', async () => {
  const env = mailEnv();
  const d = deps(env);
  // A KV that pages: two keys per page, with a cursor.
  const all = [];
  for (let n = 30; n < 35; n++) all.push((await strandedSim(env, n)).id);
  const realList = env.SOCIAL.list;
  const pages = [];
  env.SOCIAL.list = async ({ prefix, limit, cursor }) => {
    if (prefix !== 'sim:') return realList({ prefix, limit });
    const names = [...env.SOCIAL.m.keys()].filter(k => /^sim:[a-f0-9-]+$/.test(k)).sort();
    const start = cursor ? Number(cursor) : 0;
    const slice = names.slice(start, start + 2);
    pages.push(slice.length);
    const end = start + slice.length;
    return { keys: slice.map(name => ({ name })), list_complete: end >= names.length, cursor: String(end) };
  };
  const t = { ...DEFAULT_THRESHOLDS, sweep_scan_max: 2, stalled_pending_per_run: 10 };
  let total = 0;
  // 5 listings at 2 per page is exactly three ticks: one full pass.
  for (let tick = 0; tick < 3; tick++) total += await resumeStalledPending(env, d, t);
  assert.equal(total, 5, 'every listing was reached across ticks');
  assert.ok(pages.every(n => n <= 2), 'no tick read more than the scan limit');
  assert.equal(env.SOCIAL.m.has('modsweep:cursor'), false, 'the cursor is cleared once a full pass completes');
});

// ── bounded model calls, the stage checkpoint, and /submit holding its answer ──

// A call that never answers until its signal fires, the way a hung upstream does.
const hangUntilAborted = signal => new Promise((_, reject) => {
  if (signal?.aborted) return reject(signal.reason);
  signal?.addEventListener('abort', () => reject(signal.reason));
});
const tight = extra => environment({ MODERATION_THRESHOLDS: JSON.stringify({ audit_rate: 0, ...extra }) });

test('a hung classifier is recorded as a hold with a readable reason, not left classifying', async () => {
  const env = tight({ jev_timeout_ms: 25 });
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: async (url, init) => hangUntilAborted(init.signal) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held');
  assert.equal(rec.decision.reasons[0].code, 'classifier_unavailable');
  assert.match(rec.jev.error, /Jev timed out after/);
  assert.equal(d.calls.judge.length, 0, 'the judge is not asked after a classifier failure');
  assert.equal((await simOf(env, sim.id)).moderation.status, 'held');
});

test('a judge that never answers is recorded as judge_timeout and the case is held', async () => {
  const env = tight({ judge_timeout_ms: 25 });
  const sim = await publishedSim(env);
  const seen = [];
  const grok = async (body, key, opts) => { seen.push(opts); return hangUntilAborted(opts?.signal); };
  const rec = await runModerationCase(sim.id, env, deps(env, { grok }));
  assert.equal(rec.status, 'held');
  assert.equal(rec.judge.error, 'judge_timeout');
  assert.ok(rec.decision.reasons.some(r => r.code === 'judge_timeout'));
  assert.ok(seen[0].signal instanceof AbortSignal, 'the production grokFetch is handed a signal to honour');
  assert.ok(rec.timing.judge_ms >= 20 && rec.timing.total_ms >= rec.timing.judge_ms);
  assert.equal(rec.stage, 'decided');
  assert.equal(isListable(await simOf(env, sim.id)), false);
});

test('a case that is stopped inside the judge shows where, with the classifier answers and timings', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  let midRun = null;
  const grok = async body => {
    // What a person reading the KV record would see if the platform ended the run here.
    midRun = JSON.parse(env.SOCIAL.m.get(`modcase:${sim.id}`));
    return grokJudge(verdict())(body);
  };
  await runModerationCase(sim.id, env, deps(env, { grok }));
  assert.equal(midRun.status, 'classifying');
  assert.equal(midRun.stage, 'judge');
  assert.ok(midRun.jev?.answers?.content_rating, 'the classifier answers were already saved');
  assert.ok(midRun.triage, 'and the triage');
  assert.ok(Number.isFinite(midRun.timing.jev_ms) && Number.isFinite(midRun.timing.to_triage_ms));
  assert.ok(midRun.history.some(h => h.event === 'stage' && h.detail.stage === 'judge'));
  const done = await loadCase(env, sim.id);
  assert.equal(done.stage, 'decided');
  assert.ok(done.timing.judge_ms >= 0 && done.timing.total_ms >= 0);
});

// The agent-round timeout this replaces is gone with the agent. The one model call besides the judge is the researcher's.
test('a researcher that never answers is recorded and is not fatal: the judge runs without findings and the case says so', async () => {
  const env = tight({ researcher_timeout_ms: 25 });
  const sim = await publishedSim(env);
  const twin = { ...(await simOf(env, sim.id)), description: 'Category: Digital twin\nObjective: Inspect how the Port of Tacoma stages containers.\nLesson: How a container terminal moves cargo from ship to rail.\nReal-world link: A twin of the Port of Tacoma terminal from public aerial imagery.' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(twin));
  const xai = [];
  const grok = async (body, key, opts) => (body.tools ? (xai.push(opts), hangUntilAborted(opts?.signal)) : grokJudge(verdict())(body));
  const rec = await runModerationCase(sim.id, env, deps(env, { grok }));
  assert.equal(xai.length, 1, 'a Digital twin naming a real place is researched');
  assert.ok(xai[0].signal instanceof AbortSignal);
  assert.deepEqual([rec.research.ran, rec.research.trigger, rec.research.error, rec.research.findings], [true, 'real_place_or_business', 'researcher_timeout', []]);
  assert.equal(rec.status, 'approved', 'the judge ran without findings and the rules listed it');
  assert.equal(rec.decision.research.error, 'researcher_timeout');
});

test('submit holds its response until the pipeline has decided, and answers 202 only when it runs long', async () => {
  // A normal run: the caller gets the decision in the response.
  const env = environment();
  const sim = await publishedSim(env);
  const pending = [];
  const ctx = { waitUntil: p => pending.push(p) };
  const quick = await route(req('POST', `/api/simulations/${sim.id}/submit`, { token: 'user-1' }), env, deps(env), ctx);
  assert.equal(quick.status, 200);
  assert.equal((await quick.json()).status, 'listed');
  assert.equal(pending.length, 1, 'still registered with waitUntil, so a dropped client does not end the run');
  assert.equal((await loadCase(env, sim.id)).status, 'approved');

  // A slow run: past submit_wait_ms the answer is 202 and the run carries on.
  const env2 = tight({ submit_wait_ms: 20 });
  const sim2 = await publishedSim(env2);
  const pending2 = [];
  const slowJudge = async body => { await new Promise(r => setTimeout(r, 120)); return grokJudge(verdict())(body); };
  const slow = await route(req('POST', `/api/simulations/${sim2.id}/submit`, { token: 'user-1' }), env2, deps(env2, { grok: slowJudge }), { waitUntil: p => pending2.push(p) });
  assert.equal(slow.status, 202);
  assert.equal((await slow.json()).status, 'in_review');
  assert.equal((await loadCase(env2, sim2.id)).status, 'classifying', 'not decided yet at the moment it answered');
  await Promise.all(pending2);
  assert.equal((await loadCase(env2, sim2.id)).status, 'approved', 'and the background run still finished');
});

test('submit tells the author nothing about a hold beyond "in review"', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) };
  const r = await route(req('POST', `/api/simulations/${sim.id}/submit`, { token: 'user-1' }), env, d, { waitUntil: () => {} });
  const body = await r.json();
  assert.equal(r.status, 200);
  assert.equal(body.status, 'in_review');
  assert.doesNotMatch(JSON.stringify(body), /quarantin|legal|csam/i);
});

// ── /submit is a request to review, not a way to re-roll a decision ─────────

const submit = (env, sim, d, ctx = { waitUntil: () => {} }) => route(req('POST', `/api/simulations/${sim.id}/submit`, { token: 'user-1' }), env, d, ctx);

test('an author cannot lift a quarantine by submitting again', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const q = { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) };
  await runModerationCase(sim.id, env, q);
  assert.equal((await simOf(env, sim.id)).moderation.status, 'quarantined');
  const r = await submit(env, sim, deps(env));
  assert.equal(r.status, 200);
  const body = await r.json();
  assert.equal(body.status, 'in_review');
  assert.equal(body.rerun, false);
  assert.doesNotMatch(JSON.stringify(body), /quarantin|legal|csam/i);
  const after = await simOf(env, sim.id);
  assert.equal(after.moderation.status, 'quarantined', 'the listing record still carries the hold');
  assert.equal(canServe(after, 'user-1', false), false, 'and the author still cannot fetch the content');
  assert.equal((await loadCase(env, sim.id)).status, 'quarantined');
});

test('a decided listing is not re-judged by submitting the same content again', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const reject = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) };
  await runModerationCase(sim.id, env, reject);
  assert.equal((await loadCase(env, sim.id)).status, 'rejected');
  const calls = [];
  const watching = { ...deps(env), fetch: jevFetch(jevAnswers(), calls) };   // would approve if it were asked
  const r = await submit(env, sim, watching);
  const body = await r.json();
  assert.equal(r.status, 200);
  assert.deepEqual([body.status, body.rerun], ['not_listed', false]);
  assert.equal(calls.length, 0, 'no model was called');
  assert.equal(watching.calls.judge.length, 0);
  assert.equal((await loadCase(env, sim.id)).status, 'rejected');
  assert.equal((await simOf(env, sim.id)).moderation.status, 'rejected');
  // An approved listing stays listed too, instead of dropping to pending while it is re-read.
  const ok = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-0000000000e1', pak: 'PAK-OK' });
  await runModerationCase(ok.id, env, deps(env));
  const r2 = await submit(env, ok, watching);
  assert.deepEqual([(await r2.json()).rerun, (await simOf(env, ok.id)).moderation.status], [false, 'approved']);
});

test('changed content is reviewed again: the upload paths reset the listing to pending', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) });
  const changed = await simOf(env, sim.id);
  changed.moderation = { ...changed.moderation, status: 'pending' };          // what a world commit or Space upload does
  changed.moderation.submitted_at = new Date(Date.now() - 3600e3).toISOString();
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(changed));
  const r = await submit(env, sim, deps(env));
  const body = await r.json();
  assert.deepEqual([r.status, body.status, body.rerun], [200, 'listed', true]);
});

test('a review that is still running is left alone, and one that is stuck can be asked for again', async () => {
  const env = environment();
  const fresh = await publishedSim(env);
  const f = await simOf(env, fresh.id);
  f.moderation = { status: 'classifying', updated_at: new Date(Date.now() - 60e3).toISOString() };
  await env.SOCIAL.put(`sim:${fresh.id}`, JSON.stringify(f));
  env.SOCIAL.m.set(`modcase:${fresh.id}`, JSON.stringify({ sim_id: fresh.id, author_id: 'user-1', status: 'classifying', classifying_since: f.moderation.updated_at, history: [] }));
  const calls = [];
  const r = await submit(env, fresh, { ...deps(env), fetch: jevFetch(jevAnswers(), calls) });
  assert.deepEqual([(await r.json()).rerun, calls.length], [false, 0], 'a minute old: it may well be mid-run');

  const stuck = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-0000000000e2', pak: 'PAK-STUCK' });
  const s = await simOf(env, stuck.id);
  s.moderation = { status: 'classifying', updated_at: new Date(Date.now() - 10 * 60e3).toISOString(), submitted_at: new Date(Date.now() - 10 * 60e3).toISOString() };
  s.updated_at = s.moderation.updated_at;
  await env.SOCIAL.put(`sim:${stuck.id}`, JSON.stringify(s));
  const r2 = await submit(env, stuck, deps(env));
  const body = await r2.json();
  assert.deepEqual([body.rerun, body.status], [true, 'listed']);
});

test('a person holding a case is not re-run, and the answer is the same as for a quarantine', async () => {
  const env = environment();
  const held = await publishedSim(env);
  await runModerationCase(held.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) });
  assert.equal((await loadCase(env, held.id)).status, 'held');
  const calls = [];
  const r = await submit(env, held, { ...deps(env), fetch: jevFetch(jevAnswers(), calls) });
  const body = await r.json();
  assert.deepEqual([body.status, body.rerun, calls.length], ['in_review', false, 0]);
  assert.equal((await loadCase(env, held.id)).status, 'held');
});

test('submits are throttled per listing and per author per day', async () => {
  const env = environment({ MODERATION_THRESHOLDS: JSON.stringify({ audit_rate: 0, submit_min_gap_seconds: 30, submits_per_author_per_day: 3 }) });
  const sim = await publishedSim(env);
  const d = deps(env);
  assert.equal((await submit(env, sim, d)).status, 200);
  // A second request inside the gap, even though it would be refused anyway as decided:
  // make it eligible again so the throttle, not the gate, is what answers.
  const again = async () => { const s = await simOf(env, sim.id); s.moderation = { ...s.moderation, status: 'pending' }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s)); };
  await again();
  const quick = await submit(env, sim, d);
  assert.equal(quick.status, 429);
  assert.ok(Number(quick.headers.get('Retry-After')) > 0);
  // Past the gap, until the daily cap. The last request is kept in R2 as well as on the listing, so both are moved back.
  const rewind = async () => {
    const s = await simOf(env, sim.id);
    s.moderation = { ...s.moderation, status: 'pending', submitted_at: new Date(Date.now() - 3600e3).toISOString() };
    await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
    const key = `locks/state/${sim.id}.json`;
    const state = JSON.parse(new TextDecoder().decode(env.SCENES.m.get(key).bytes));
    await env.SCENES.put(key, JSON.stringify({ ...state, last_submit_at: Date.now() - 3600e3 }));
  };
  for (let n = 0; n < 2; n++) {
    await rewind();
    assert.equal((await submit(env, sim, d)).status, 200, `submit ${n + 2}`);
  }
  await rewind();
  const capped = await submit(env, sim, d);
  assert.equal(capped.status, 429, 'the fourth run in a day is refused');
});

test('content changed under a hard-category hold is not re-run by the AI: the hold stays with a person', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) });
  assert.equal((await loadCase(env, sim.id)).decision.lane, 'legal');
  // An edit and republish: the upload path resets the listing to pending.
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'pending', submitted_at: new Date(Date.now() - 3600e3).toISOString() };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  const calls = [];
  const r = await submit(env, sim, { ...deps(env), fetch: jevFetch(jevAnswers(), calls) });
  const body = await r.json();
  assert.deepEqual([r.status, body.status, body.rerun, calls.length], [200, 'in_review', false, 0]);
  assert.equal((await simOf(env, sim.id)).moderation.status, 'held', 'the hold marker is put back on the listing');
  assert.equal((await loadCase(env, sim.id)).status, 'held');
  assert.ok((await loadCase(env, sim.id)).history.some(h => h.event === 'resubmitted_while_held'), 'a reviewer can see the content changed');
  assert.equal(isListable(await simOf(env, sim.id)), false);
});

// ── Security review: the case is the authority, evidence is write-once, runs are serialised ──

const LEASE_KEY = id => `locks/moderation/${id}.json`;
const evidence = async (env, id, path) => { const o = await env.SCENES.get(`universes/${id}/moderation/${path}`); return o ? new TextDecoder().decode(await o.arrayBuffer()) : null; };
const PNG8 = validPng(4);
const putCapture = (env, id, n, bytes = PNG8, d = deps(env)) => route(req('PUT', `/api/simulations/${id}/captures/${n}`, { token: 'user-1', body: bytes }), env, d);
const putDossier = (env, id, d = deps(env)) => route(req('PUT', `/api/simulations/${id}/dossier`, { token: 'user-1', body: JSON.stringify(dossier({ content_root: 'blake3:new' })) }), env, d);

test('evidence cannot be overwritten under a legal hold: dossier and captures answer 409 and say nothing about why', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) });
  assert.equal((await loadCase(env, sim.id)).status, 'quarantined');
  const before = await evidence(env, sim.id, 'dossier.json');
  for (const r of [await putDossier(env, sim.id), await putCapture(env, sim.id, 0)]) {
    assert.equal(r.status, 409);
    const body = await r.json();
    assert.equal(body.code, 'in_review');
    assert.doesNotMatch(JSON.stringify(body), /quarantin|legal|csam|hold|frozen/i);
  }
  assert.equal(await evidence(env, sim.id, 'dossier.json'), before, 'the stored dossier is untouched');
});

test('evidence is locked while a review reads it and after it is decided, until the content changes', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  // Mid-run: the listing says classifying.
  const s = await simOf(env, sim.id); s.moderation = { status: 'classifying' }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await putCapture(env, sim.id, 0)).status, 409, 'a review in flight reads these');
  // Decided: the captures are what the verdict was drawn from.
  s.moderation = { status: 'rejected' }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await putDossier(env, sim.id)).status, 409);
  assert.equal((await putCapture(env, sim.id, 0)).status, 409);
  // Changed content resets the listing to pending, and only then may new evidence arrive.
  s.moderation = { status: 'pending' }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await putCapture(env, sim.id, 0)).status, 200);
});

test('new evidence after a decision goes to a new epoch and the verdict\'s evidence is kept', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const first = await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) });
  assert.equal(first.status, 'rejected');
  assert.equal(first.evidence_epoch, 0);
  const oldCapture = await evidence(env, sim.id, 'capture-0.png');
  assert.ok(oldCapture);
  // The author changes the content and republishes.
  const s = await simOf(env, sim.id); s.moderation = { ...s.moderation, status: 'pending' }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await putCapture(env, sim.id, 0, validPng(9))).status, 200);
  assert.equal(await evidence(env, sim.id, 'capture-0.png'), oldCapture, 'the rejected verdict\'s capture is still there');
  assert.ok(await evidence(env, sim.id, 'e1/capture-0.png'), 'the new one is in the next epoch');
  assert.equal((await putDossier(env, sim.id)).status, 200);
  assert.ok(await evidence(env, sim.id, 'e1/dossier.json'));
  // The next review reads the new epoch and says so; the old one stays on the old case's record.
  const second = await runModerationCase(sim.id, env, deps(env), { trigger: 'rerun' });
  assert.equal(second.evidence_epoch, 1);
  assert.deepEqual(second.judge.captures_used, ['capture-0.png']);
  assert.equal((await simOf(env, sim.id)).moderation.evidence_epoch, 1);
});

test('submits fired at once start one run, not twelve, whatever the throttle says', async () => {
  const env = tight({ submit_min_gap_seconds: 30, submits_per_author_per_day: 3 });
  const sim = await publishedSim(env);
  const calls = [];
  const d = { ...deps(env), fetch: jevFetch(jevAnswers(), calls) };
  const answers = await Promise.all(Array.from({ length: 12 }, () => submit(env, sim, d)));
  const bodies = await Promise.all(answers.map(r => r.json()));
  assert.equal(calls.length, 1, 'one classifier call');
  assert.equal(d.calls.judge.length, 1, 'one judge call');
  assert.equal(bodies.filter(b => b.rerun === true).length, 1);
  assert.ok(answers.every(r => r.status === 200 || r.status === 429));
  assert.equal([...env.SCENES.m.keys()].filter(k => k.startsWith(`locks/submit/user-1/${new Date().toISOString().slice(0, 10)}/`)).length, 1, 'and it was counted once');
});

test('the daily cap holds across listings fired at once', async () => {
  const env = tight({ submit_min_gap_seconds: 1, submits_per_author_per_day: 3 });
  const sims = [];
  for (let n = 0; n < 8; n++) sims.push(await publishedSim(env, { id: `a1b2c3d4-0000-4000-8000-0000000007${String(n).padStart(2, '0')}`, pak: `PAK-CAP-${n}` }));
  const d = deps(env);
  const answers = await Promise.all(sims.map(s => submit(env, s, d)));
  assert.equal(answers.filter(r => r.status === 429).length, 5, '8 listings, a cap of 3: five are refused');
  assert.equal(d.calls.judge.length, 3);
});

test('a run whose lease was taken over cannot overwrite the verdict of the run that replaced it', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  let release; const gate = new Promise(r => { release = r; });
  const slowReject = async body => { await gate; return grokJudge(rejectVerdict())(body); };
  const first = runModerationCase(sim.id, env, deps(env, { grok: slowReject }), { trigger: 'publish' });
  await new Promise(r => setTimeout(r, 20));
  assert.ok(env.SCENES.m.has(LEASE_KEY(sim.id)), 'the running case holds a lease');
  // A second run while the lease is live does nothing.
  const calls = [];
  const blocked = await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers(), calls) }, { trigger: 'sweep' });
  assert.equal(calls.length, 0, 'no second run while the first holds its lease');
  assert.equal(blocked.status, 'classifying');
  // The lease expires (the first run is presumed dead), and a second run takes over and decides.
  const stale = JSON.parse(new TextDecoder().decode(env.SCENES.m.get(LEASE_KEY(sim.id)).bytes));
  await env.SCENES.put(LEASE_KEY(sim.id), JSON.stringify({ ...stale, expires_at: Date.now() - 1000 }));
  const second = await runModerationCase(sim.id, env, deps(env), { trigger: 'sweep' });
  assert.equal(second.status, 'approved');
  // The first run wakes up and tries to write its own verdict.
  release();
  await first;
  const final = await loadCase(env, sim.id);
  assert.equal(final.status, 'approved', 'the later run\'s verdict stands');
  assert.equal(final.judge.verdict, 'publish');
  assert.equal((await simOf(env, sim.id)).moderation.status, 'approved');
  assert.equal(env.SCENES.m.has(LEASE_KEY(sim.id)), false, 'and the lease is released');
});

test('a case a person was asked to decide is not given back to the AI by a content change', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), grokFetch: grokJudge(verdict({ verdict: 'flag_for_human_review', confidence: 0.4 })) });
  const held = await loadCase(env, sim.id);
  assert.equal(held.status, 'held');
  assert.notEqual(held.decision.lane, 'legal');
  const s = await simOf(env, sim.id); s.moderation = { ...s.moderation, status: 'pending', submitted_at: new Date(Date.now() - 3600e3).toISOString() }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  const calls = [];
  const body = await (await submit(env, sim, { ...deps(env), fetch: jevFetch(jevAnswers(), calls) })).json();
  assert.deepEqual([body.status, body.rerun, calls.length], ['in_review', false, 0]);
  assert.equal((await loadCase(env, sim.id)).status, 'held');
  assert.equal((await simOf(env, sim.id)).moderation.status, 'held');
  assert.ok((await loadCase(env, sim.id)).history.some(h => h.event === 'resubmitted_while_held'));
});

test('a hold caused by the AI failing, not by its doubt, can be drawn again', async () => {
  const env = tight({ judge_timeout_ms: 20 });
  const sim = await publishedSim(env);
  const hang = async (body, key, opts) => new Promise((_, rej) => opts.signal.addEventListener('abort', () => rej(opts.signal.reason)));
  await runModerationCase(sim.id, env, deps(env, { grok: hang }));
  assert.equal((await loadCase(env, sim.id)).decision.reasons.at(-1).code, 'judge_timeout');
  const s = await simOf(env, sim.id); s.moderation = { ...s.moderation, status: 'pending', submitted_at: new Date(Date.now() - 3600e3).toISOString() }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  const r = await submit(env, sim, deps(env));
  assert.deepEqual([(await r.json()).rerun, (await loadCase(env, sim.id)).status], [true, 'approved']);
});

test('after three non-approved verdicts in a day the next change goes to a person, not the AI', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const reject = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) };
  for (let n = 0; n < 3; n++) {
    await runModerationCase(sim.id, env, reject, { trigger: 'rerun' });
    const s = await simOf(env, sim.id); s.moderation = { ...s.moderation, status: 'pending', submitted_at: new Date(Date.now() - 3600e3).toISOString() }; await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  }
  const calls = [];
  const r = await submit(env, sim, { ...deps(env), fetch: jevFetch(jevAnswers(), calls) });
  const body = await r.json();
  assert.deepEqual([r.status, body.status, body.rerun, calls.length], [200, 'in_review', false, 0]);
  const rec = await loadCase(env, sim.id);
  assert.equal(rec.status, 'held');
  assert.ok(rec.decision.reasons.some(x => x.code === 'review_limit'));
});

// ── leases, epochs and the budget, one level down ──

test('a verdict is not applied to content that changed while the run was working', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  // While the judge works, the author uploads another package: the upload path
  // resets the listing to pending and replaces the etag.
  const swap = async body => {
    const s = await simOf(env, sim.id);
    s.pak_etag = 'a-different-package'; s.scene_size_bytes += 1; s.moderation = { ...s.moderation, status: 'pending' };
    await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
    return grokJudge(verdict())(body);
  };
  const rec = await runModerationCase(sim.id, env, deps(env, { grok: swap }));
  assert.equal(rec.status, 'pending', 'the approval was not applied');
  assert.equal((await simOf(env, sim.id)).moderation.status, 'pending');
  assert.equal(isListable(await simOf(env, sim.id)), false);
  assert.ok(rec.history.some(h => h.event === 'content_changed_during_review'));
  // The new content gets its own review.
  const again = await runModerationCase(sim.id, env, deps(env), { trigger: 'rerun' });
  assert.equal(again.status, 'approved');
});

test('the sweep does not hand a case a person was asked to decide back to the AI', async () => {
  const env = mailEnv();
  const flagged = await strandedSim(env, 30);
  env.SOCIAL.m.set(`modcase:${flagged.id}`, JSON.stringify({ sim_id: flagged.id, author_id: 'user-1', status: 'held', decision: { lane: 'judge', reasons: [{ code: 'judge_flagged' }] }, history: [] }));
  assert.equal(await resumeStalledPending(env, deps(env)), 0);
  assert.equal((await loadCase(env, flagged.id)).status, 'held');
  // The same hold, caused by the AI failing to answer, may be drawn again.
  const failed = await strandedSim(env, 31);
  env.SOCIAL.m.set(`modcase:${failed.id}`, JSON.stringify({ sim_id: failed.id, author_id: 'user-1', status: 'held', decision: { lane: 'judge', reasons: [{ code: 'judge_timeout' }] }, history: [] }));
  assert.equal(await resumeStalledPending(env, deps(env)), 1);
  assert.equal((await loadCase(env, failed.id)).status, 'approved');
});

test('old submit slots and long-expired leases are removed, a live lease is kept', async () => {
  const env = environment();
  const day = n => new Date(Date.now() - n * 86400000).toISOString().slice(0, 10);
  await env.SCENES.put(`locks/submit/user-1/${day(3)}/0`, '{}');
  await env.SCENES.put(`locks/submit/user-1/${day(0)}/0`, '{}');
  await env.SCENES.put('locks/moderation/dead.json', JSON.stringify({ holder: 'x', expires_at: Date.now() - 2 * 3600e3 }));
  await env.SCENES.put('locks/moderation/live.json', JSON.stringify({ holder: 'y', expires_at: Date.now() + 60e3 }));
  assert.equal(await sweepLocks(env), 2);
  assert.deepEqual([...env.SCENES.m.keys()].sort(), [`locks/moderation/live.json`, `locks/submit/user-1/${day(0)}/0`]);
});

test('two runs taking over one expired lease: exactly one wins, and only the holder releases it', async () => {
  const env = environment();
  const key = 'locks/moderation/sim-x.json';
  const first = await acquireLease(env, 'sim-x', DEFAULT_THRESHOLDS, 'a');
  assert.ok(first);
  assert.equal(await acquireLease(env, 'sim-x', DEFAULT_THRESHOLDS, 'b'), null, 'a live lease is not shared');
  const stale = JSON.parse(new TextDecoder().decode(env.SCENES.m.get(key).bytes));
  await env.SCENES.put(key, JSON.stringify({ ...stale, expires_at: Date.now() - 1 }));
  const [c, d] = await Promise.all([acquireLease(env, 'sim-x', DEFAULT_THRESHOLDS, 'c'), acquireLease(env, 'sim-x', DEFAULT_THRESHOLDS, 'd')]);
  assert.equal([c, d].filter(Boolean).length, 1);
  await releaseLease(env, first);
  assert.ok(env.SCENES.m.has(key), 'a run that lost its lease does not release the new holder\'s');
  await releaseLease(env, c || d);
  assert.equal(env.SCENES.m.has(key), false);
});

test('a tool call from a run whose lease was taken over does nothing', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const lease = await acquireLease(env, sim.id, DEFAULT_THRESHOLDS, 'run');
  const exec = makeToolExecutor(env, deps(env), { actor: 'agent', actorId: 'grok-agent', context: 'escalation', lease });
  assert.equal((await exec('moderation_get_case', { sim_id: sim.id })).error, 'no case', 'while it holds the lease the call goes through');
  await env.SCENES.delete(`locks/moderation/${sim.id}.json`);
  const r = await exec('moderation_get_case', { sim_id: sim.id });
  assert.equal(r.ok, false);
  assert.match(r.error, /newer run/);
});

test('a finished run leaves no lease behind, and a decided listing can be reviewed again', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, deps(env));
  assert.equal(env.SCENES.m.has(`locks/moderation/${sim.id}.json`), false);
});

test('the admin capture viewer opens the epoch the verdict was drawn from, and another on request', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, deps(env));
  await env.SCENES.put(`universes/${sim.id}/moderation/e1/capture-0.png`, new Uint8Array([0x89, 0x50, 0x4e, 0x47, 7, 7, 7, 7]).buffer, { httpMetadata: { contentType: 'image/png' } });
  const d = deps(env, { admin: 'admin-1' });
  const fetchCapture = async q => new Uint8Array(await (await route(req('GET', `/api/admin/moderation/captures/${sim.id}/0${q}`, { token: 'admin-1' }), env, d)).arrayBuffer());
  assert.notEqual((await fetchCapture(''))[4], 7, 'the capture the verdict saw');
  assert.equal((await fetchCapture('?epoch=1'))[4], 7, 'the later epoch, when asked for');
});

test('an infrastructure hold and an admin re-run are not verdicts: neither counts against the listing', async () => {
  const env = tight({ judge_timeout_ms: 20 });
  const sim = await publishedSim(env);
  const hang = async (body, key, opts) => new Promise((_, rej) => opts.signal.addEventListener('abort', () => rej(opts.signal.reason)));
  for (let n = 0; n < 4; n++) await runModerationCase(sim.id, env, deps(env, { grok: hang }), { trigger: 'rerun' });
  const held = await loadCase(env, sim.id);
  assert.equal(held.status, 'held');
  assert.equal(held.ai_verdicts?.length ?? 0, 0, 'the AI failing to answer is not a draw');
  const reject = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) };
  await runModerationCase(sim.id, env, reject, { trigger: 'rerun', actorId: 'admin-1' });
  assert.equal((await loadCase(env, sim.id)).status, 'rejected');
  assert.equal((await loadCase(env, sim.id)).ai_verdicts?.length ?? 0, 0, 'an admin\'s re-run is not the AI re-rolling');
});

test('evidence is refused for a case held in the legal lane, even when the listing reads pending; a freeze from before no longer refuses it', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  env.SOCIAL.m.set(`modcase:${sim.id}`, JSON.stringify({ sim_id: sim.id, author_id: 'user-1', status: 'held', decision: { lane: 'legal', outcome: 'hold' }, history: [] }));
  assert.equal((await putCapture(env, sim.id, 0)).status, 409);
  assert.equal((await putDossier(env, sim.id)).status, 409);
  env.SOCIAL.m.delete(`modcase:${sim.id}`);
  assert.equal((await putCapture(env, sim.id, 0)).status, 200, 'control: without the hold the same write is accepted');
  // A publish freeze a quarantine wrote before 2026-10-15 is not read: the account is never what locks a listing.
  await env.USERS.put('publish-frozen:user-1', JSON.stringify({ sim_id: 'another' }));
  assert.equal((await putCapture(env, sim.id, 1)).status, 200);
  assert.equal((await putDossier(env, sim.id)).status, 200);
});

test('a quarantined case whose listing was reset to pending is not reviewed, and its marker is put back', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) });
  assert.equal((await loadCase(env, sim.id)).status, 'quarantined');
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'pending', submitted_at: new Date(Date.now() - 3600e3).toISOString() };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  const calls = [];
  const r = await submit(env, sim, { ...deps(env), fetch: jevFetch(jevAnswers(), calls) });
  const body = await r.json();
  assert.deepEqual([r.status, body.status, body.rerun, calls.length], [200, 'in_review', false, 0]);
  assert.equal((await simOf(env, sim.id)).moderation.status, 'quarantined', 'the hold marker is put back');
  assert.equal((await loadCase(env, sim.id)).status, 'quarantined');
  assert.ok((await loadCase(env, sim.id)).history.some(h => h.event === 'resubmitted_while_quarantined'));
});

test('a request that waited for the lease decides again on what the earlier run left', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = deps(env);
  let release; const gate = new Promise(r => { release = r; });
  const realPut = env.SCENES.put;
  let waited = false;
  env.SCENES.put = async (key, body, opts) => {
    if (!waited && key === `locks/moderation/${sim.id}.json`) { waited = true; await gate; }
    return realPut(key, body, opts);
  };
  // It reads the listing as ready for a review, then waits for its lease...
  const waiting = submit(env, sim, d);
  await new Promise(r => setTimeout(r, 20));
  env.SCENES.put = realPut;
  // ...while another run takes the lease, decides, and releases it.
  assert.equal((await runModerationCase(sim.id, env, d)).status, 'approved');
  release();
  const body = await (await waiting).json();
  assert.deepEqual([body.status, body.rerun], ['listed', false], 'what the earlier run decided stands: nothing new to review');
  assert.equal(d.calls.judge.length, 1, 'the judge ran once');
});

// ── round 21: the world in the fingerprint, R2 state, the legal lock ──

const lockKind = (env, id) => { const o = env.SCENES.m.get(`locks/legal/${id}`); return o ? (o.customMetadata?.kind ?? null) : null; };

test('the content fingerprint tells two worlds of the same size apart', () => {
  const world = (sha, hash) => ({ id: 'w', format: 'echk', pak_etag: null, scene_size_bytes: 5000, r2_key: 'universes/w/world/manifest.json', world: { bytes: 5000, manifest_sha256: sha, publish_hash: hash } });
  const a = 'a'.repeat(64), b = 'b'.repeat(64);
  assert.notEqual(contentFingerprint(world(a, 'h1')), contentFingerprint(world(b, 'h1')));
  assert.notEqual(contentFingerprint(world(a, 'h1')), contentFingerprint(world(a, 'h2')));
  assert.equal(contentFingerprint(world(a, 'h1')), contentFingerprint(world(a, 'h1')));
});

test('a verdict is not applied to a world swapped for another of the same size', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const first = await simOf(env, sim.id);
  Object.assign(first, { format: 'echk', pak_etag: null, world: { bytes: first.scene_size_bytes, manifest_sha256: 'a'.repeat(64), publish_hash: 'h1' } });
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(first));
  const swap = async body => {
    const s = await simOf(env, sim.id);
    s.world = { ...s.world, manifest_sha256: 'b'.repeat(64), publish_hash: 'h2' };
    s.moderation = { ...s.moderation, status: 'pending' };
    await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
    return grokJudge(verdict())(body);
  };
  const rec = await runModerationCase(sim.id, env, deps(env, { grok: swap }));
  assert.equal(rec.status, 'pending', 'the approval drawn on world A is not applied to world B');
  assert.equal(isListable(await simOf(env, sim.id)), false);
});

test('a review seals its evidence epoch in R2, so a stale copy of the case cannot reopen it', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, deps(env));
  const sealed = await evidence(env, sim.id, 'capture-0.png');
  assert.ok(sealed);
  // Another location still sees a listing and a case that carry no epoch.
  const s = await simOf(env, sim.id);
  s.moderation = { status: 'pending' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  env.SOCIAL.m.set(`modcase:${sim.id}`, JSON.stringify({ sim_id: sim.id, author_id: 'user-1', status: 'pending', history: [] }));
  assert.equal((await putCapture(env, sim.id, 0, validPng(9))).status, 200);
  assert.equal(await evidence(env, sim.id, 'capture-0.png'), sealed, 'the sealed epoch is untouched');
  assert.ok(await evidence(env, sim.id, 'e1/capture-0.png'), 'the new capture goes to the next epoch');
});

test('the gap between review requests cannot be bypassed by an older copy of the listing', async () => {
  const env = tight({ submit_min_gap_seconds: 30, submits_per_author_per_day: 5 });
  const sim = await publishedSim(env);
  const d = deps(env);
  assert.equal((await submit(env, sim, d)).status, 200);
  // Another location reads the listing before it carries the time of the request.
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'pending' };
  delete s.moderation.submitted_at;
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await submit(env, sim, d)).status, 429);
});

test('a quarantine writes the legal lock, a release removes it, and a hold in the legal lane gets the lesser kind', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) });
  assert.equal(lockKind(env, sim.id), 'serve_block');
  const exec = makeToolExecutor(env, deps(env, { admin: 'admin-1' }), { actor: 'admin', actorId: 'admin-1' });
  const released = await exec('moderation_release', { sim_id: sim.id, rationale: 'A person reviewed it: not what the classifier suspected.', rating: 'teen_13' });
  assert.equal(released.ok, true, JSON.stringify(released));
  assert.equal(env.SCENES.m.has(`locks/legal/${sim.id}`), false, 'the lock goes with the hold');
  const held = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-000000000009', pak: 'PAK-HELD' });
  await runModerationCase(held.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) });
  assert.equal(lockKind(env, held.id), 'content_lock');
});

test('a hold applied at another location is seen through the lock even when this copy of the case is old', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await env.SCENES.put(`locks/legal/${sim.id}`, JSON.stringify({ kind: 'serve_block' }), { customMetadata: { kind: 'serve_block' } });
  assert.equal(await contentLockedFor(env, await simOf(env, sim.id), 'user-1'), true);
  assert.equal((await putCapture(env, sim.id, 0)).status, 409);
});

test('the lock sweep gives a case quarantined before locks existed its lock, writes no case, and never removes a lock', async () => {
  const env = environment();
  const q = await publishedSim(env);
  const legacy = JSON.stringify({ sim_id: q.id, author_id: 'user-1', status: 'quarantined', legal_hold: { category: 'csam_or_minor_sexualization' }, history: [] });
  env.SOCIAL.m.set(`modcase:${q.id}`, legacy);
  env.SOCIAL.m.set(`modq:quarantined:${q.id}`, '1');
  const stray = 'a1b2c3d4-0000-4000-8000-000000000008';
  await env.SCENES.put(`locks/legal/${stray}`, '{}', { customMetadata: { kind: 'serve_block' } });
  await sweepLocks(env);
  assert.equal(lockKind(env, q.id), 'serve_block');
  assert.equal(env.SOCIAL.m.get(`modcase:${q.id}`), legacy, 'the sweep does not write the case');
  assert.equal(env.SCENES.m.has(`locks/legal/${stray}`), true, 'a lock whose case reads as not locked is for a person to lift, not the sweep');
});

// ── round 22: only a person lifts a legal lock; an approval is for the reviewed content ──

const quarantined = async env => {
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) });
  assert.equal(lockKind(env, sim.id), 'serve_block');
  return sim;
};
const admin = env => makeToolExecutor(env, deps(env, { admin: 'admin-1' }), { actor: 'admin', actorId: 'admin-1' });

test('a write that carries no legal state cannot drop a lock, and cannot weaken it', async () => {
  const env = environment();
  const sim = await quarantined(env);
  // A run, the agent or the sweep read the case before the hold was applied.
  const stale = await loadCase(env, sim.id);
  Object.assign(stale, { legal_hold: null, status: 'approved' });
  await assert.rejects(saveCase(env, stale), LegalLockConflict);
  assert.equal(lockKind(env, sim.id), 'serve_block', 'the lock is still there');
  const weaker = await loadCase(env, sim.id);
  Object.assign(weaker, { legal_hold: null, status: 'held', decision: { lane: 'legal', outcome: 'hold' } });
  await assert.rejects(saveCase(env, weaker), LegalLockConflict, 'serve_block does not become content_lock');
  assert.equal(lockKind(env, sim.id), 'serve_block');
  assert.equal((await loadCase(env, sim.id)).status, 'quarantined', 'and neither stale write reached the case');
});

test('only a person lifts a lock, and the same release can be run again if the removal failed', async () => {
  const env = environment();
  const sim = await quarantined(env);
  const realDelete = env.SCENES.delete;
  env.SCENES.delete = async key => { if (key.startsWith('locks/legal/')) throw new Error('r2 unavailable'); return realDelete(key); };
  const args = { sim_id: sim.id, rationale: 'A person reviewed it: not what the classifier suspected.', rating: 'teen_13' };
  const failed = await admin(env)('moderation_release', args);
  assert.equal(failed.ok, false, 'the admin sees the failure');
  assert.match(failed.error, /r2 unavailable/);
  assert.equal(lockKind(env, sim.id), 'serve_block', 'the lock is still there');
  env.SCENES.delete = realDelete;
  assert.equal((await admin(env)('moderation_release', args)).ok, true, 'running the release again removes it');
  assert.equal(env.SCENES.m.has(`locks/legal/${sim.id}`), false);
});

test('a hold applied during a run is not dropped by the run: it stops, and the lock and the hold stand', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  // While the judge works, an admin escalates a legal hold.
  const escalate = async body => {
    const r = await admin(env)('moderation_escalate_legal', { sim_id: sim.id, category: 'terrorism_or_extremist_promotion', note: 'Reported by a user.' });
    assert.equal(r.ok, true, JSON.stringify(r));
    return grokJudge(verdict())(body);
  };
  await runModerationCase(sim.id, env, deps(env, { grok: escalate }));
  const rec = await loadCase(env, sim.id);
  assert.ok(rec.legal_hold, 'the escalated hold is still on the case');
  assert.equal(lockKind(env, sim.id), 'serve_block');
  assert.notEqual((await simOf(env, sim.id)).moderation.status, 'approved', 'the run did not approve it');
});

test('a lock write that fails stops the case from being saved, and the admin sees why', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, deps(env));
  const realPut = env.SCENES.put;
  env.SCENES.put = async (key, ...rest) => { if (key.startsWith('locks/legal/')) throw new Error('r2 unavailable'); return realPut(key, ...rest); };
  const r = await admin(env)('moderation_quarantine', { sim_id: sim.id, category: 'csam_or_minor_sexualization', rationale: 'A person judged this to be what the classifier suspected.' });
  assert.equal(r.ok, false);
  assert.match(r.error, /r2 unavailable/);
  assert.equal((await loadCase(env, sim.id)).status, 'approved', 'the case was not saved as quarantined without its lock');
});

test('the pipeline\'s hard-category hold leaves the listing unserved when the lock cannot be written', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const realPut = env.SCENES.put;
  env.SCENES.put = async (key, ...rest) => { if (key.startsWith('locks/legal/')) throw new Error('r2 unavailable'); return realPut(key, ...rest); };
  await runModerationCase(sim.id, env, { ...deps(env), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.95 })) });
  assert.equal(isListable(await simOf(env, sim.id)), false, 'never approved');
});

test('a person approving a case whose content changed after the review is told to re-run it first', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, { ...deps(env), grokFetch: grokJudge(verdict({ verdict: 'flag_for_human_review', confidence: 0.4 })) });
  assert.equal((await loadCase(env, sim.id)).status, 'held');
  // The author uploads something else while the case waits for a person.
  const s = await simOf(env, sim.id);
  s.pak_etag = 'another-package'; s.moderation = { ...s.moderation, status: 'pending' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  const approve = { sim_id: sim.id, rating: 'teen_13', rationale: 'Looks fine to me after reading the case.' };
  const refused = await admin(env)('moderation_approve', approve);
  assert.equal(refused.ok, false);
  assert.match(refused.error, /changed after this case was reviewed/);
  assert.equal(isListable(await simOf(env, sim.id)), false);
  // After a re-run on the new content, the person's decision applies.
  await runModerationCase(sim.id, env, { ...deps(env), grokFetch: grokJudge(verdict({ verdict: 'flag_for_human_review', confidence: 0.4 })) }, { trigger: 'rerun', actorId: 'admin-1' });
  assert.equal((await admin(env)('moderation_approve', approve)).ok, true);
  assert.equal(isListable(await simOf(env, sim.id)), true);
});

test('a route write that would drop a lock answers like ordinary review', async () => {
  const env = environment();
  const sim = await quarantined(env);
  // A stale copy of the case, as a read at another location might return it: rejected, no hold.
  const stale = await loadCase(env, sim.id);
  Object.assign(stale, { status: 'rejected', legal_hold: null, decision: { lane: 'quality', outcome: 'reject' } });
  env.SOCIAL.m.set(`modcase:${sim.id}`, JSON.stringify(stale));
  const r = await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'Please look at this again, it is fine.' }) }), env, deps(env), { waitUntil() {} });
  assert.equal(r.status, 409);
  const body = await r.json();
  assert.equal(body.code, 'in_review');
  assert.doesNotMatch(JSON.stringify(body), /quarantin|legal|lock|hold/i);
  assert.equal(lockKind(env, sim.id), 'serve_block');
});

// ── round 23: an approval is bound to the objects that were reviewed ──

const approvalOf = (env, id) => { const o = env.SCENES.m.get(`locks/approved/${id}.json`); return o ? JSON.parse(new TextDecoder().decode(o.bytes)) : null; };
// An object's identity as the binding records it: its SHA-256 when it has one, else its etag.
const etagOfKey = (env, key) => { const o = env.SCENES.m.get(key); return !o ? null : o.checksums?.sha256 ? `sha256:${Buffer.from(o.checksums.sha256).toString('hex')}` : `etag:${o.etag}:v${o.version}`; };
const thumbKey = id => `thumbnails/${id}/thumb.png`;
const putThumb = (env, id, text) => env.SCENES.put(thumbKey(id), text, { httpMetadata: { contentType: 'image/png' } });

test('an approval binds the package and the thumbnail that were reviewed, and the judge saw the thumbnail', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await putThumb(env, sim.id, 'thumb-v1');
  const rec = await runModerationCase(sim.id, env, deps(env));
  assert.equal(rec.status, 'approved');
  const b = approvalOf(env, sim.id);
  assert.deepEqual([b.content, b.thumb, b.source], [etagOfKey(env, sim.r2_key), etagOfKey(env, thumbKey(sim.id)), null]);
  assert.ok(rec.judge.captures_used.includes('listing thumbnail (the image the Gallery shows)'));
});

test('a package swapped in storage while the run works is not approved, though the listing record did not change', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const swap = async body => { await env.SCENES.put(sim.r2_key, new TextEncoder().encode('SWAPPED-PACKAGE').buffer); return grokJudge(verdict())(body); };
  const rec = await runModerationCase(sim.id, env, deps(env, { grok: swap }));
  assert.equal(rec.status, 'pending');
  assert.equal(approvalOf(env, sim.id), null, 'nothing was bound');
  assert.equal(isListable(await simOf(env, sim.id)), false);
});

test('a thumbnail changed while the run works is not approved either', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await putThumb(env, sim.id, 'thumb-v1');
  const swap = async body => { await putThumb(env, sim.id, 'thumb-v2'); return grokJudge(verdict())(body); };
  const rec = await runModerationCase(sim.id, env, deps(env, { grok: swap }));
  assert.equal(rec.status, 'pending');
  assert.equal(approvalOf(env, sim.id), null);
});

test('an approved case that moves to any other status revokes the binding, and a re-run binds the new objects', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, deps(env));
  const first = approvalOf(env, sim.id);
  assert.ok(first && !first.revoked);
  const rejected = await admin(env)('moderation_reject', { sim_id: sim.id, lane: 'quality', category: 'low_effort_test', rationale: 'A person judged this to be a test upload with nothing in it.' });
  assert.equal(rejected.ok, true, JSON.stringify(rejected));
  assert.equal(approvalOf(env, sim.id).revoked, true, 'a takedown does not depend on the marker');
  // New content, reviewed again: the binding names the new package.
  await env.SCENES.put(sim.r2_key, new TextEncoder().encode('PAKBYTES-2').buffer);
  const s = await simOf(env, sim.id);
  s.pak_etag = etagOfKey(env, sim.r2_key); s.moderation = { ...s.moderation, status: 'pending' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await runModerationCase(sim.id, env, deps(env), { trigger: 'rerun', actorId: 'admin-1' })).status, 'approved');
  assert.equal(approvalOf(env, sim.id).content, etagOfKey(env, sim.r2_key));
});

test('/submit reviews an approved listing whose thumbnail changed, and not one that did not change', async () => {
  const env = tight({ submit_min_gap_seconds: 0, submits_per_author_per_day: 8 });
  const sim = await publishedSim(env);
  await putThumb(env, sim.id, 'thumb-v1');
  const d = deps(env);
  assert.equal((await (await submit(env, sim, d)).json()).rerun, true, 'the first review');
  assert.equal(approvalOf(env, sim.id).thumb, etagOfKey(env, thumbKey(sim.id)));
  assert.equal((await (await submit(env, sim, d)).json()).rerun, false, 'nothing changed');
  // The author replaces the image; the listing record still says approved.
  await putThumb(env, sim.id, 'thumb-v2');
  assert.equal((await simOf(env, sim.id)).moderation.status, 'approved');
  assert.equal((await (await submit(env, sim, d)).json()).rerun, true, 'the new image is reviewed');
  assert.equal(approvalOf(env, sim.id).thumb, etagOfKey(env, thumbKey(sim.id)), 'and bound');
});

test('the sweep binds a listing approved before bindings existed', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'approved' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  env.SOCIAL.m.set(`modq:approved:${sim.id}`, '1');
  assert.equal(approvalOf(env, sim.id), null);
  await sweepLocks(env);
  assert.equal(approvalOf(env, sim.id).content, etagOfKey(env, sim.r2_key));
});

// ── round 24: a thumbnail alone is reviewed without taking the listing down ──

test('a thumbnail-only change is re-reviewed with the listing still listed, and a review that fails takes it down', async () => {
  const env = tight({ submit_min_gap_seconds: 0, submits_per_author_per_day: 8 });
  const sim = await publishedSim(env);
  await putThumb(env, sim.id, 'thumb-v1');
  assert.equal((await (await submit(env, sim, deps(env))).json()).rerun, true);
  assert.equal((await simOf(env, sim.id)).moderation.status, 'approved');
  // The author replaces the image only.
  await putThumb(env, sim.id, 'thumb-v2');
  let during = null;
  const watch = async body => {
    during = { marker: (await simOf(env, sim.id)).moderation.status, binding: approvalOf(env, sim.id) };
    return grokJudge(verdict())(body);
  };
  const ran = await (await submit(env, sim, deps(env, { grok: watch }))).json();
  assert.equal(ran.rerun, true);
  assert.equal(during.marker, 'approved', 'the listing stayed listed while the new image was reviewed');
  assert.ok(during.binding && !during.binding.revoked, 'and its binding stood');
  assert.equal(approvalOf(env, sim.id).thumb, etagOfKey(env, thumbKey(sim.id)), 'the new image is bound once approved');
  // A new image that fails its review takes the listing down.
  await putThumb(env, sim.id, 'thumb-v3');
  await submit(env, sim, deps(env, { judge: rejectVerdict() }));
  assert.equal((await simOf(env, sim.id)).moderation.status, 'rejected');
  assert.equal(approvalOf(env, sim.id).revoked, true, 'and nothing binds it any more');
});

test('a changed package still takes the listing down while it is reviewed', async () => {
  const env = tight({ submit_min_gap_seconds: 0, submits_per_author_per_day: 8 });
  const sim = await publishedSim(env);
  assert.equal((await (await submit(env, sim, deps(env))).json()).rerun, true);
  // The package is replaced in storage; the listing record still says approved.
  await env.SCENES.put(sim.r2_key, new TextEncoder().encode('PAKBYTES-2').buffer);
  let marker = null;
  const watch = async body => { marker = (await simOf(env, sim.id)).moderation.status; return grokJudge(verdict())(body); };
  await submit(env, sim, deps(env, { grok: watch }));
  assert.notEqual(marker, 'approved', 'new content is not served while it is reviewed');
});

// ── round 25: the creator cover is bound like the thumbnail; approved captures can be previewed ──

const coverKey = id => `covers/${id}/cover.png`;
const putCover = (env, id, text) => env.SCENES.put(coverKey(id), text, { httpMetadata: { contentType: 'image/png' } });

test('a cover is bound like the thumbnail, and the judge sees it', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await putThumb(env, sim.id, 'thumb-v1');
  await putCover(env, sim.id, 'cover-v1');
  const rec = await runModerationCase(sim.id, env, deps(env));
  assert.equal(rec.status, 'approved');
  assert.equal(approvalOf(env, sim.id).cover, etagOfKey(env, coverKey(sim.id)));
  assert.ok(rec.judge.captures_used.includes('listing cover (the box art the Gallery shows)'));
  assert.equal(await coverApproved(env, await simOf(env, sim.id)), true);
  await putCover(env, sim.id, 'cover-v2');
  assert.equal(await coverApproved(env, await simOf(env, sim.id)), false, 'a changed cover is not the reviewed one');
  assert.equal(await coverApproved(env, { id: 'a-listing-with-no-binding' }), true, 'a listing from before bindings has no record to differ from');
});

test('a binding from before covers existed reads as no cover, and a cover alone is reviewed with the listing still listed', async () => {
  const env = tight({ submit_min_gap_seconds: 0, submits_per_author_per_day: 8 });
  const sim = await publishedSim(env);
  assert.equal((await (await submit(env, sim, deps(env))).json()).rerun, true);
  const old = approvalOf(env, sim.id);
  delete old.cover;
  await env.SCENES.put(`locks/approved/${sim.id}.json`, JSON.stringify(old));
  assert.equal((await (await submit(env, sim, deps(env))).json()).rerun, false, 'no cover, nothing new');
  await putCover(env, sim.id, 'cover-v1');
  let marker = null;
  const watch = async body => { marker = (await simOf(env, sim.id)).moderation.status; return grokJudge(verdict())(body); };
  assert.equal((await (await submit(env, sim, deps(env, { grok: watch }))).json()).rerun, true, 'the new cover is reviewed');
  assert.equal(marker, 'approved', 'the listing stayed listed while it was');
  assert.equal(approvalOf(env, sim.id).cover, etagOfKey(env, coverKey(sim.id)));
});

test('approved captures are offered for preview only while the approval holds, and only those of the approved epoch', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const keysOf = async () => (await approvedCaptureKeys(env, await simOf(env, sim.id))).map(k => `${k.n}.${k.ext}`);
  assert.deepEqual(await keysOf(), [], 'not approved yet');
  await runModerationCase(sim.id, env, deps(env));
  assert.deepEqual(await keysOf(), ['0.png', '1.png'], 'the captures the approved review read');
  assert.equal(approvalOf(env, sim.id).evidence_epoch, 0);
  // The author republishes with new captures: they go to the next epoch and are not offered until a review approves them.
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'pending' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await putCapture(env, sim.id, 0, validPng(9))).status, 200);
  assert.equal((await putDossier(env, sim.id)).status, 200);
  assert.deepEqual(await keysOf(), [], 'a pending listing offers nothing');
  await runModerationCase(sim.id, env, deps(env), { trigger: 'rerun', actorId: 'admin-1' });
  assert.equal(approvalOf(env, sim.id).evidence_epoch, 1);
  assert.deepEqual(await keysOf(), ['0.png'], 'now the new epoch, which holds one capture');
  // A legal lock, a revoked binding, a swapped package and a private listing each offer nothing.
  await env.SCENES.put(`locks/legal/${sim.id}`, '{}', { customMetadata: { kind: 'serve_block' } });
  assert.deepEqual(await keysOf(), []);
  await env.SCENES.delete(`locks/legal/${sim.id}`);
  assert.deepEqual(await keysOf(), ['0.png']);
  await env.SCENES.put(sim.r2_key, new TextEncoder().encode('SWAPPED').buffer);
  assert.deepEqual(await keysOf(), [], 'a package that is not the reviewed one');
});

test('a revoked binding and a private listing offer no preview captures', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  await runModerationCase(sim.id, env, deps(env));
  const raw = await simOf(env, sim.id);
  assert.equal((await approvedCaptureKeys(env, { ...raw, is_public: false })).length, 0, 'a private listing is not previewed to the public');
  const rejected = await admin(env)('moderation_reject', { sim_id: sim.id, lane: 'quality', category: 'low_effort_test', rationale: 'A person judged this to be a test upload with nothing in it.' });
  assert.equal(rejected.ok, true);
  // A stale approved marker is written back over the takedown.
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify({ ...raw, moderation: { ...raw.moderation, status: 'approved' } }));
  assert.deepEqual(await approvedCaptureKeys(env, await simOf(env, sim.id)), []);
});

// A PNG with a real header (1280x720), so the capture and thumbnail routes accept it; `seed` makes its bytes differ.
function validPng(seed = 0) {
  const chunk = (name, data) => { const out = new Uint8Array(12 + data.length); new DataView(out.buffer).setUint32(0, data.length); out.set(new TextEncoder().encode(name), 4); out.set(data, 8); return out; };
  const ihdr = new Uint8Array(13);
  const view = new DataView(ihdr.buffer);
  view.setUint32(0, 1280);
  view.setUint32(4, 720);
  ihdr.set([8, 2, 0, 0, 0], 8);
  const parts = [new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', new Uint8Array([seed & 255, 1, 2, 3])), chunk('IEND', new Uint8Array(0))];
  const bytes = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { bytes.set(p, at); at += p.length; }
  return bytes;
}

// ── round 26: SHA-256 binding, fail closed where required, user images ──

const hexOfBytes = bytes => Buffer.from(bytes).toString('hex');
const sha = bytes => createHash('sha256').update(Buffer.from(bytes)).digest('hex');

test('an MD5-colliding swap is caught by the SHA-256 the object carries', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const original = new TextEncoder().encode('PAKBYTES-1');
  await env.SCENES.put(sim.r2_key, original.buffer, { sha256: sha(original) });
  await runModerationCase(sim.id, env, deps(env));
  assert.equal(approvalOf(env, sim.id).content, `sha256:${sha(original)}`, 'bound by the hash R2 holds, not the etag');
  assert.equal((await approvalMovement(env, await simOf(env, sim.id))).moved, false);
  // A different package that carries the SAME etag (a collision), and its own SHA-256, which R2 verified when it was stored.
  const etag = env.SCENES.m.get(sim.r2_key).etag;
  const other = new TextEncoder().encode('A-DIFFERENT-PACKAGE');
  await env.SCENES.put(sim.r2_key, other.buffer, { sha256: sha(other) });
  env.SCENES.m.get(sim.r2_key).etag = etag;
  assert.equal(await etagOfKey(env, sim.r2_key), `sha256:${sha(other)}`);
  assert.equal((await approvalMovement(env, await simOf(env, sim.id))).moved, true, 'the etag is the same, the hash is not');
  assert.equal(await isListable(await simForServing(env, await simOf(env, sim.id), 'content')), false);
});

test('captures that went through the capture route carry a SHA-256, and a preview is offered only while each is the bound object', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const png = validPng(21);
  await runModerationCase(sim.id, env, deps(env));
  // The author republishes; the capture route stores the new capture, with its SHA-256, in the next epoch.
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'pending' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal((await putCapture(env, sim.id, 3, png)).status, 200);
  const stored = env.SCENES.m.get(`universes/${sim.id}/moderation/e1/capture-3.png`);
  assert.ok(stored.checksums?.sha256, 'R2 was given the SHA-256 to verify');
  assert.equal(hexOfBytes(stored.checksums.sha256), sha(new Uint8Array(stored.bytes)));
  assert.equal((await putDossier(env, sim.id)).status, 200);
  await runModerationCase(sim.id, env, deps(env), { trigger: 'rerun', actorId: 'admin-1' });
  assert.deepEqual((await approvedCaptureKeys(env, await simOf(env, sim.id))).map(k => k.n), [3]);
  // The object under that key is replaced after the approval (the epoch is write-once only if nothing writes into it).
  await env.SCENES.put(`universes/${sim.id}/moderation/e1/capture-3.png`, validPng(22).buffer, { httpMetadata: { contentType: 'image/png' } });
  assert.deepEqual(await approvedCaptureKeys(env, await simOf(env, sim.id)), [], 'no longer the bound object');
});

test('where bindings are required, an approved marker with no binding reads as pending, and only a person binds it', async () => {
  const env = environment({ REQUIRE_APPROVAL_BINDING: 'true' });
  const sim = await publishedSim(env);
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'approved' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  env.SOCIAL.m.set(`modq:approved:${sim.id}`, '1');
  env.SOCIAL.m.set(`modcase:${sim.id}`, JSON.stringify({ sim_id: sim.id, author_id: 'user-1', status: 'approved', history: [] }));
  const listed = async () => isListable(await simForServing(env, await simOf(env, sim.id), 'content'));
  assert.equal(await listed(), false, 'nothing reviewed is bound');
  assert.deepEqual([await thumbnailApproved(env, s), await coverApproved(env, s), await sourceApproved(env, s)], [false, false, false]);
  await sweepLocks(env);
  assert.equal(approvalOf(env, sim.id), null, 'the sweep binds nothing here');
  // Not the agent, and not a mistake: a person, with a reason.
  assert.equal((await makeToolExecutor(env, deps(env), { actor: 'agent', actorId: 'grok-agent' })('moderation_bind', { sim_id: sim.id, rationale: 'The agent should not be able to do this.' })).ok, false);
  assert.equal((await makeToolExecutor(env, deps(env), { actor: 'system', actorId: 'pipeline' })('moderation_bind', { sim_id: sim.id, rationale: 'The pipeline should not be able to do this.' })).ok, false);
  const rationale = 'A person opened the package and the captures and they are what was approved.';
  const shown = await admin(env)('moderation_bind', { sim_id: sim.id, rationale });
  assert.equal(shown.ok && shown.bound === false, true, 'without expected it only shows what is stored');
  assert.equal(approvalOf(env, sim.id), null);
  const bound = await admin(env)('moderation_bind', { sim_id: sim.id, rationale, expected: shown.objects });
  assert.equal(bound.ok, true, JSON.stringify(bound));
  assert.equal(await listed(), true);
  assert.equal(approvalOf(env, sim.id).by, 'admin:admin-1');
  assert.equal((await admin(env)('moderation_bind', { sim_id: sim.id, rationale: 'short' })).ok, false, 'a rationale is required');
});

test('where bindings are not required, a listing from before them serves and the sweep binds it', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'approved' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  assert.equal(await isListable(await simForServing(env, await simOf(env, sim.id), 'content')), true);
  assert.equal(await thumbnailApproved(env, s), true);
});

// ── round 27: dedup follows the SHA-256, R2's version identifies an unhashed object, bind what was shown ──

const COLLISION_IDS = ['a1b2c3d4-0000-4000-8000-000000000771', 'a1b2c3d4-0000-4000-8000-000000000772', 'a1b2c3d4-0000-4000-8000-000000000773'];

test('two different packages with one etag are not one package: dedup follows the SHA-256', async () => {
  const env = environment();
  const calls = [];
  const d = { ...deps(env), fetch: jevFetch(jevAnswers(), calls) };
  const benign = await publishedSim(env, { id: COLLISION_IDS[0], pak: 'BENIGN-PAK-BYTES' });
  assert.equal((await runModerationCase(benign.id, env, d)).status, 'approved');
  assert.equal(calls.length, 1);
  // A twin with another SHA-256 that carries the same etag and size (an MD5 collision): R2 verified the hash it stored.
  const twin = await publishedSim(env, { id: COLLISION_IDS[1], pak: 'HARMFUL-PAK-BYTES' });
  env.SCENES.m.get(twin.r2_key).etag = env.SCENES.m.get(benign.r2_key).etag;
  const s = await simOf(env, twin.id);
  Object.assign(s, { pak_etag: env.SCENES.m.get(benign.r2_key).etag, scene_size_bytes: (await simOf(env, benign.id)).scene_size_bytes });
  await env.SOCIAL.put(`sim:${twin.id}`, JSON.stringify(s));
  const rec = await runModerationCase(twin.id, env, d);
  assert.equal(calls.length, 2, 'the twin was reviewed, not given the first one\'s verdict');
  assert.equal(rec.decision?.dedup_of, undefined);
  // The same bytes still share a verdict.
  const same = await publishedSim(env, { id: COLLISION_IDS[2], pak: 'BENIGN-PAK-BYTES' });
  const copied = await runModerationCase(same.id, env, d);
  assert.equal(calls.length, 2, 'identical bytes are not reviewed again');
  assert.equal(copied.decision.dedup_of, benign.id);
});

test('a package with no SHA-256 (assembled from parts) is always reviewed, never copied', async () => {
  const env = environment();
  const calls = [];
  const d = { ...deps(env), fetch: jevFetch(jevAnswers(), calls) };
  for (const [n, id] of [[0, COLLISION_IDS[0]], [1, COLLISION_IDS[1]]]) {
    const sim = await publishedSim(env, { id, pak: 'ONE-PACKAGE-IN-PARTS' });
    delete env.SCENES.m.get(sim.r2_key).checksums;
    env.SCENES.m.get(sim.r2_key).version = 'one-version'; // same etag and version: only the missing SHA-256 keeps them apart
    assert.equal((await runModerationCase(sim.id, env, d)).status, 'approved', `listing ${n}`);
  }
  assert.equal(calls.length, 2, 'each was reviewed');
});

test('an object with no SHA-256 is identified by its etag and R2\'s version, so a swap that keeps the etag still moves', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  delete env.SCENES.m.get(sim.r2_key).checksums;
  await runModerationCase(sim.id, env, deps(env));
  const stored = env.SCENES.m.get(sim.r2_key);
  assert.equal(approvalOf(env, sim.id).content, `etag:${stored.etag}:v${stored.version}`);
  assert.equal((await approvalMovement(env, await simOf(env, sim.id))).moved, false);
  // Another upload of the key whose etag is the same (a collision) has a new version.
  const etag = stored.etag;
  await env.SCENES.put(sim.r2_key, new TextEncoder().encode('A-DIFFERENT-ASSEMBLED-PACKAGE').buffer);
  env.SCENES.m.get(sim.r2_key).etag = etag;
  assert.equal((await approvalMovement(env, await simOf(env, sim.id))).moved, true, 'same etag, new version');
});

test('moderation_bind shows what is stored and binds exactly what the person was shown', async () => {
  const env = environment({ REQUIRE_APPROVAL_BINDING: 'true' });
  const sim = await publishedSim(env);
  const s = await simOf(env, sim.id);
  s.moderation = { ...s.moderation, status: 'approved' };
  await env.SOCIAL.put(`sim:${sim.id}`, JSON.stringify(s));
  env.SOCIAL.m.set(`modcase:${sim.id}`, JSON.stringify({ sim_id: sim.id, author_id: 'user-1', status: 'approved', history: [] }));
  const rationale = 'A person opened the package and the captures and they are what was approved.';
  const shown = await admin(env)('moderation_bind', { sim_id: sim.id, rationale });
  assert.equal(shown.ok && shown.bound === false, true, 'without expected it only shows what is stored');
  assert.match(shown.objects.content, /^sha256:/);
  assert.equal(approvalOf(env, sim.id), null);
  // The art is swapped between looking and binding.
  await putThumb(env, sim.id, 'a-different-image');
  const refused = await admin(env)('moderation_bind', { sim_id: sim.id, rationale, expected: shown.objects });
  assert.equal(refused.ok, false);
  assert.match(refused.error, /not the ones you were shown/);
  assert.equal(approvalOf(env, sim.id), null, 'nothing was bound');
  // Look again, bind what is shown.
  const again = await admin(env)('moderation_bind', { sim_id: sim.id, rationale });
  const bound = await admin(env)('moderation_bind', { sim_id: sim.id, rationale, expected: again.objects });
  assert.equal(bound.ok, true, JSON.stringify(bound));
  assert.equal(approvalOf(env, sim.id).thumb, again.objects.thumb);
});

test('the binding report counts approved listings that are bound and not, and only a person may ask', async () => {
  const env = environment({ REQUIRE_APPROVAL_BINDING: 'true' });
  const bound = await publishedSim(env, { id: COLLISION_IDS[0] });
  await runModerationCase(bound.id, env, deps(env));
  const loose = await publishedSim(env, { id: COLLISION_IDS[1], pak: 'ANOTHER-PACKAGE' });
  const s = await simOf(env, loose.id);
  s.moderation = { ...s.moderation, status: 'approved' };
  await env.SOCIAL.put(`sim:${loose.id}`, JSON.stringify(s));
  env.SOCIAL.m.set(`modq:approved:${loose.id}`, '1');
  const report = await admin(env)('moderation_binding_report', {});
  assert.deepEqual([report.ok, report.approved, report.bound, report.unbound, report.required], [true, 2, 1, 1, true]);
  assert.deepEqual(report.unbound_ids, [loose.id]);
  assert.equal((await makeToolExecutor(env, deps(env), { actor: 'system', actorId: 'pipeline' })('moderation_binding_report', {})).ok, false);
});

test('the content fingerprint names the package by its SHA-256 too', () => {
  const sim = hash => ({ id: 'p', pak_etag: 'etag-1', pak_sha256: hash, scene_size_bytes: 10, r2_key: 'universes/p/universe.pak' });
  assert.notEqual(contentFingerprint(sim('a'.repeat(64))), contentFingerprint(sim('b'.repeat(64))));
});
